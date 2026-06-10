//! End-to-end demo of the RAS authorization extension.
//!
//! Two generated RAS REST services (invoice + billing), an embedded RAS
//! authority issuing internal service tokens, an auth gateway narrowing
//! browser web sessions to single-audience backend tokens, and a topology
//! declaration whose generated artifacts constrain both the authority and
//! the gateway.
//!
//! ```text
//! browser ── web session ──> gateway ──/api/invoice──> invoice-service
//!                               │
//!                               └──/api/billing──> billing-service
//!                                                      │ internal token
//!                                                      ▼ (embedded authority)
//!                                                  invoice-service
//! ```

use std::sync::Arc;

use ras_auth_core::{AuthError, AuthFuture, AuthProvider};

/// Invoice service: the downstream both the gateway and billing call.
pub mod invoice_api {
    use ras_rest_macro::rest_service;
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    pub struct Invoice {
        pub id: String,
        pub customer: String,
        pub amount_cents: i64,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    pub struct InvoicesResponse {
        pub invoices: Vec<Invoice>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    pub struct CreateInvoiceRequest {
        pub customer: String,
        pub amount_cents: i64,
    }

    rest_service!({
        service_name: InvoiceService,
        base_path: "/api/invoice",
        openapi: false,
        endpoints: [
            GET WITH_PERMISSIONS(["invoice:read"]) invoices() -> InvoicesResponse,
            POST WITH_PERMISSIONS(["invoice:write"]) invoices(CreateInvoiceRequest) -> Invoice,
        ]
    });
}

/// Billing service: calls the invoice service with RAS internal tokens.
pub mod billing_api {
    use ras_rest_macro::rest_service;
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    pub struct BillingSummary {
        pub invoice_count: usize,
        pub total_cents: i64,
    }

    rest_service!({
        service_name: BillingService,
        base_path: "/api/billing",
        openapi: false,
        endpoints: [
            GET WITH_PERMISSIONS(["billing:read"]) summary() -> BillingSummary,
        ]
    });
}

// The logical topology: services, the public gateway, and the one allowed
// service-to-service edge. The generated `authorization_demo_topology()`
// validates the graph and emits the policy/profile artifacts that the
// authority and gateway load below.
ras_topology_macro::ras_topology!({
    topology_name: AuthorizationDemo,

    services: [
        invoice: {
            audience: "invoice-service",
            manifest: crate::invoice_api::generate_invoiceservice_permission_manifest,
            exposure: private,
        },
        billing: {
            audience: "billing-service",
            manifest: crate::billing_api::generate_billingservice_permission_manifest,
            exposure: private,
        },
    ],

    gateways: [
        public_web: {
            exposure: public,
            routes: [
                "/api/invoice" => invoice { expose_private },
                "/api/billing" => billing { expose_private },
            ],
        },
    ],

    calls: [
        billing -> invoice {
            permissions: [
                crate::invoice_api::invoiceservice_permissions::INVOICE_READ,
            ],
        },
    ],
});

/// Accepts a token if any inner provider does (e.g. RAS internal tokens
/// *or* gateway-derived tokens). Providers are tried in order; the last
/// error wins when all fail.
pub struct MultiTokenAuthProvider {
    providers: Vec<Box<dyn AuthProvider>>,
}

impl MultiTokenAuthProvider {
    pub fn new(providers: Vec<Box<dyn AuthProvider>>) -> Self {
        Self { providers }
    }
}

impl AuthProvider for MultiTokenAuthProvider {
    fn authenticate(&self, token: String) -> AuthFuture<'_> {
        Box::pin(async move {
            let mut last_error = AuthError::InvalidToken;
            for provider in &self.providers {
                match provider.authenticate(token.clone()).await {
                    Ok(user) => return Ok(user),
                    Err(err) => last_error = err,
                }
            }
            Err(last_error)
        })
    }
}

/// Demo wiring shared by the binary and the integration tests.
pub mod demo {
    use std::sync::Arc;

    use ras_authorization_core::{
        AudiencePermission, InMemoryAuditSink, InMemoryAuthorizationStore, Principal,
        RasTokenAuthProvider, ServiceGraphPolicy, ServiceIdentityProof, ServiceRegistration,
        StaticSecretVerifier, TokenIssuer,
    };
    use ras_authorization_gateway::backend_validation_options;
    use ras_authorization_token::{
        AudiencePolicy, JwkSet, SigningKey, TokenType, TokenValidator, ValidationOptions,
    };
    use ras_integration_core::{IntegrationConfig, TokenManager, TokenRequest, TokenSubject};
    use ras_integration_ras::{EmbeddedAuthority, RasInternalTokenSource};
    use ras_rest_core::{RestError, RestResponse, RestResult};
    use ras_transport_core::HttpTransport;
    use tokio::sync::Mutex;

    use crate::billing_api::{BillingServiceBuilder, BillingServiceTrait, BillingSummary};
    use crate::invoice_api::{
        CreateInvoiceRequest, Invoice, InvoiceServiceBuilder, InvoiceServiceClient,
        InvoiceServiceTrait, InvoicesResponse,
    };
    use crate::{MultiTokenAuthProvider, authorization_demo_topology};

    pub const AUTHORITY_ISSUER: &str = "https://auth.internal";
    pub const GATEWAY_ISSUER: &str = "https://gateway.internal";
    pub const BILLING_SECRET: &str = "billing-service-demo-secret-32-bytes!!";

    /// The embedded authority: registry, grants, issuer — constrained by
    /// the topology's generated policy artifact.
    pub struct Authority {
        pub issuer: Arc<TokenIssuer>,
        pub store: Arc<InMemoryAuthorizationStore>,
        pub verifier: Arc<StaticSecretVerifier>,
        pub audit: Arc<InMemoryAuditSink>,
    }

    /// Build the authority from the topology: register every declared
    /// service, import its manifest, grant the declared edge, and load the
    /// generated policy.
    pub async fn build_authority() -> Authority {
        let topology = authorization_demo_topology().expect("topology must validate");

        let store = Arc::new(InMemoryAuthorizationStore::new());
        let verifier = Arc::new(StaticSecretVerifier::new());
        let audit = Arc::new(InMemoryAuditSink::new());

        for service in topology.services() {
            store
                .register_service(ServiceRegistration {
                    service_id: service.id.clone(),
                    display_name: service.id.clone(),
                    audience: service.audience.clone(),
                    enabled: true,
                })
                .await
                .expect("service registration");
            store
                .import_manifest(&service.audience, &service.manifest)
                .await
                .expect("manifest import");
        }
        verifier
            .register("billing", BILLING_SECRET.as_bytes())
            .await
            .expect("verifier registration");

        // Grants mirror the declared topology edge.
        store
            .grant(
                Principal::Service {
                    service_id: "billing".to_string(),
                },
                AudiencePermission::new("invoice-service", "invoice:read"),
            )
            .await
            .expect("grant");

        let issuer = Arc::new(
            TokenIssuer::builder(
                AUTHORITY_ISSUER,
                SigningKey::generate_es256("authority-1"),
                store.clone(),
                verifier.clone(),
            )
            .audit(audit.clone())
            .build(),
        );

        // The generated policy artifact constrains issuance to declared
        // edges.
        let policy: ServiceGraphPolicy =
            serde_json::from_str(&topology.authz_policy_json().expect("policy artifact"))
                .expect("policy artifact loads");
        issuer.load_policy(policy).await;

        Authority {
            issuer,
            store,
            verifier,
            audit,
        }
    }

    /// In-memory invoice service implementation.
    pub struct InvoiceServiceImpl {
        invoices: Mutex<Vec<Invoice>>,
    }

    impl Default for InvoiceServiceImpl {
        fn default() -> Self {
            Self {
                invoices: Mutex::new(vec![
                    Invoice {
                        id: "inv-1".to_string(),
                        customer: "acme".to_string(),
                        amount_cents: 12_50,
                    },
                    Invoice {
                        id: "inv-2".to_string(),
                        customer: "globex".to_string(),
                        amount_cents: 99_00,
                    },
                ]),
            }
        }
    }

    #[async_trait::async_trait]
    impl InvoiceServiceTrait for InvoiceServiceImpl {
        async fn get_invoices(
            &self,
            _user: &ras_auth_core::AuthenticatedUser,
        ) -> RestResult<InvoicesResponse> {
            Ok(RestResponse::ok(InvoicesResponse {
                invoices: self.invoices.lock().await.clone(),
            }))
        }

        async fn post_invoices(
            &self,
            _user: &ras_auth_core::AuthenticatedUser,
            request: CreateInvoiceRequest,
        ) -> RestResult<Invoice> {
            let mut invoices = self.invoices.lock().await;
            let invoice = Invoice {
                id: format!("inv-{}", invoices.len() + 1),
                customer: request.customer,
                amount_cents: request.amount_cents,
            };
            invoices.push(invoice.clone());
            Ok(RestResponse::ok(invoice))
        }
    }

    /// Build the invoice router. It accepts RAS internal tokens (from
    /// services like billing) and gateway-derived tokens (from browser
    /// traffic) — both single-audience, both enforced by the generated
    /// permission requirements.
    pub fn build_invoice_router(authority_jwks: JwkSet, gateway_jwks: JwkSet) -> axum::Router {
        let internal = RasTokenAuthProvider::new(TokenValidator::new(
            authority_jwks,
            ValidationOptions::new(
                AUTHORITY_ISSUER,
                AudiencePolicy::Exact("invoice-service".to_string()),
                vec![TokenType::InternalService],
            ),
        ));
        let from_gateway = RasTokenAuthProvider::new(TokenValidator::new(
            gateway_jwks,
            backend_validation_options(GATEWAY_ISSUER, "invoice-service"),
        ));
        InvoiceServiceBuilder::new(InvoiceServiceImpl::default())
            .auth_provider(MultiTokenAuthProvider::new(vec![
                Box::new(internal),
                Box::new(from_gateway),
            ]))
            .build()
    }

    /// Billing service implementation: serves `/summary` by calling the
    /// invoice service with a RAS-issued internal token.
    pub struct BillingServiceImpl {
        token_manager: Arc<TokenManager>,
        invoice_client: InvoiceServiceClient,
    }

    impl BillingServiceImpl {
        pub fn new(token_manager: Arc<TokenManager>, invoice_client: InvoiceServiceClient) -> Self {
            Self {
                token_manager,
                invoice_client,
            }
        }
    }

    #[async_trait::async_trait]
    impl BillingServiceTrait for BillingServiceImpl {
        async fn get_summary(
            &self,
            _user: &ras_auth_core::AuthenticatedUser,
        ) -> RestResult<BillingSummary> {
            // Acquire (or reuse from cache) an internal token for the
            // invoice-service audience, then call the generated client.
            let lease = self
                .token_manager
                .get_token(TokenRequest {
                    integration_id: "invoice-service".to_string(),
                    subject: TokenSubject::Service,
                    scopes: vec!["invoice:read".to_string()],
                    audience: Some("invoice-service".to_string()),
                    force_refresh: false,
                })
                .await
                .map_err(|err| {
                    RestError::internal_server_error(format!("token acquisition: {err}"))
                })?;

            let mut client = self.invoice_client.clone();
            client.set_bearer_token(Some(lease.access_token.expose_secret()));
            let invoices = client
                .get_invoices()
                .await
                .map_err(|err| RestError::internal_server_error(format!("invoice call: {err}")))?;

            Ok(RestResponse::ok(BillingSummary {
                invoice_count: invoices.invoices.len(),
                total_cents: invoices
                    .invoices
                    .iter()
                    .map(|invoice| invoice.amount_cents)
                    .sum(),
            }))
        }
    }

    /// Build the billing router: accepts gateway-derived tokens for its own
    /// audience, and acquires internal tokens via the embedded authority to
    /// call the invoice service.
    pub fn build_billing_router(
        authority: &Authority,
        gateway_jwks: JwkSet,
        invoice_base_url: &str,
        invoice_transport: Arc<dyn HttpTransport>,
    ) -> axum::Router {
        let source = Arc::new(RasInternalTokenSource::new(
            Arc::new(EmbeddedAuthority::new(authority.issuer.clone())),
            ServiceIdentityProof {
                service_id: "billing".to_string(),
                proof: serde_json::json!({ "client_secret": BILLING_SECRET }),
            },
        ));
        let token_manager = Arc::new(
            TokenManager::builder()
                .register(
                    IntegrationConfig::new("invoice-service", ["invoice:read"], [invoice_base_url])
                        .expect("integration config")
                        .with_allowed_audiences(["invoice-service"]),
                    source,
                )
                .expect("integration registration")
                .build(),
        );
        let invoice_client = InvoiceServiceClient::builder(invoice_base_url)
            .build_with_transport(invoice_transport)
            .expect("invoice client");

        let from_gateway = RasTokenAuthProvider::new(TokenValidator::new(
            gateway_jwks,
            backend_validation_options(GATEWAY_ISSUER, "billing-service"),
        ));
        BillingServiceBuilder::new(BillingServiceImpl::new(token_manager, invoice_client))
            .auth_provider(from_gateway)
            .build()
    }
}

pub use demo::{AUTHORITY_ISSUER, BILLING_SECRET, GATEWAY_ISSUER};
pub type SharedAuthProvider = Arc<dyn AuthProvider>;
