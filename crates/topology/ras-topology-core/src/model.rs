//! The topology model and its build-time validation.

use std::collections::{BTreeMap, BTreeSet};

use ras_permission_manifest::{PermissionManifest, ServicePermissions};

use crate::error::TopologyError;

/// Anything usable as a service's permission manifest in a topology:
/// either a combined [`PermissionManifest`] or a single generated
/// [`ServicePermissions`] (what `generate_*_permission_manifest` functions
/// return).
pub trait IntoManifest {
    fn into_manifest(self) -> PermissionManifest;
}

impl IntoManifest for PermissionManifest {
    fn into_manifest(self) -> PermissionManifest {
        self
    }
}

impl IntoManifest for ServicePermissions {
    fn into_manifest(self) -> PermissionManifest {
        PermissionManifest::from_services([self])
    }
}

/// Whether a node may be reached from outside the deployment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exposure {
    Public,
    Private,
}

/// A declared service: a logical node with an audience and its generated
/// permission manifest.
#[derive(Debug, Clone)]
pub struct ServiceNode {
    pub id: String,
    pub audience: String,
    pub exposure: Exposure,
    /// The service's generated permission manifest; edge permissions are
    /// validated against it.
    pub manifest: PermissionManifest,
}

/// One gateway route: path prefix → declared service.
#[derive(Debug, Clone)]
pub struct RouteDecl {
    pub prefix: String,
    pub service_id: String,
    /// Allow requests with no permissions for the audience (forwarded to
    /// generated gateway profiles).
    pub authenticated_only: bool,
    /// Explicitly allow a public gateway to expose a private service.
    pub expose_private: bool,
}

impl RouteDecl {
    pub fn new(prefix: impl Into<String>, service_id: impl Into<String>) -> Self {
        Self {
            prefix: prefix.into(),
            service_id: service_id.into(),
            authenticated_only: false,
            expose_private: false,
        }
    }

    pub fn authenticated_only(mut self) -> Self {
        self.authenticated_only = true;
        self
    }

    pub fn expose_private(mut self) -> Self {
        self.expose_private = true;
        self
    }
}

/// A declared gateway profile.
#[derive(Debug, Clone)]
pub struct GatewayNode {
    pub id: String,
    pub exposure: Exposure,
    pub routes: Vec<RouteDecl>,
}

/// A declared service-to-service call edge.
#[derive(Debug, Clone)]
pub struct CallEdge {
    pub caller_id: String,
    pub target_id: String,
    pub permissions: BTreeSet<String>,
    /// Whether the permissions were declared through the explicit custom
    /// path (skipping manifest validation).
    pub custom: bool,
}

/// A validated topology. Construct through [`Topology::builder`].
#[derive(Debug, Clone)]
pub struct Topology {
    pub(crate) name: String,
    pub(crate) services: Vec<ServiceNode>,
    pub(crate) gateways: Vec<GatewayNode>,
    pub(crate) calls: Vec<CallEdge>,
}

impl Topology {
    pub fn builder(name: impl Into<String>) -> TopologyBuilder {
        TopologyBuilder {
            name: name.into(),
            services: Vec::new(),
            gateways: Vec::new(),
            calls: Vec::new(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn services(&self) -> &[ServiceNode] {
        &self.services
    }

    pub fn gateways(&self) -> &[GatewayNode] {
        &self.gateways
    }

    pub fn calls(&self) -> &[CallEdge] {
        &self.calls
    }

    pub(crate) fn service(&self, id: &str) -> Option<&ServiceNode> {
        self.services.iter().find(|service| service.id == id)
    }
}

/// Builder with deterministic build-time validation.
pub struct TopologyBuilder {
    name: String,
    services: Vec<ServiceNode>,
    gateways: Vec<GatewayNode>,
    calls: Vec<CallEdge>,
}

impl TopologyBuilder {
    /// Declare a service with its audience, exposure, and generated
    /// permission manifest.
    pub fn service(
        mut self,
        id: impl Into<String>,
        audience: impl Into<String>,
        exposure: Exposure,
        manifest: impl IntoManifest,
    ) -> Self {
        self.services.push(ServiceNode {
            id: id.into(),
            audience: audience.into(),
            exposure,
            manifest: manifest.into_manifest(),
        });
        self
    }

    /// Declare a gateway profile with its routes.
    pub fn gateway(
        mut self,
        id: impl Into<String>,
        exposure: Exposure,
        routes: Vec<RouteDecl>,
    ) -> Self {
        self.gateways.push(GatewayNode {
            id: id.into(),
            exposure,
            routes,
        });
        self
    }

    /// Declare an allowed service-to-service call edge. Permissions are
    /// validated against the target service's manifest at build time.
    pub fn call(
        mut self,
        caller_id: impl Into<String>,
        target_id: impl Into<String>,
        permissions: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.calls.push(CallEdge {
            caller_id: caller_id.into(),
            target_id: target_id.into(),
            permissions: permissions.into_iter().map(Into::into).collect(),
            custom: false,
        });
        self
    }

    /// Declare a call edge whose permissions are *not* validated against
    /// the target manifest. Explicitly named so manual permission strings
    /// stay visible.
    pub fn call_with_custom_permissions(
        mut self,
        caller_id: impl Into<String>,
        target_id: impl Into<String>,
        permissions: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.calls.push(CallEdge {
            caller_id: caller_id.into(),
            target_id: target_id.into(),
            permissions: permissions.into_iter().map(Into::into).collect(),
            custom: true,
        });
        self
    }

    /// Validate the graph and produce a [`Topology`].
    pub fn build(self) -> Result<Topology, TopologyError> {
        if self.name.is_empty() {
            return Err(TopologyError::Invalid(
                "topology name must not be empty".to_string(),
            ));
        }

        // Unique service ids and audiences.
        let mut audiences: BTreeMap<&str, &str> = BTreeMap::new();
        let mut service_ids: BTreeSet<&str> = BTreeSet::new();
        for service in &self.services {
            if !service_ids.insert(&service.id) {
                return Err(TopologyError::DuplicateServiceId(service.id.clone()));
            }
            if let Some(first) = audiences.insert(&service.audience, &service.id) {
                return Err(TopologyError::DuplicateAudience {
                    audience: service.audience.clone(),
                    first: first.to_string(),
                    second: service.id.clone(),
                });
            }
        }

        // Gateways: unique ids, per-gateway unique prefixes, declared
        // targets, exposure rules.
        let mut gateway_ids: BTreeSet<&str> = BTreeSet::new();
        for gateway in &self.gateways {
            if !gateway_ids.insert(&gateway.id) {
                return Err(TopologyError::DuplicateGatewayId(gateway.id.clone()));
            }
            let mut prefixes: BTreeSet<&str> = BTreeSet::new();
            for route in &gateway.routes {
                if !prefixes.insert(&route.prefix) {
                    return Err(TopologyError::DuplicateRoutePrefix {
                        gateway: gateway.id.clone(),
                        prefix: route.prefix.clone(),
                    });
                }
                let target = self
                    .services
                    .iter()
                    .find(|service| service.id == route.service_id)
                    .ok_or_else(|| TopologyError::UnknownRouteTarget {
                        gateway: gateway.id.clone(),
                        prefix: route.prefix.clone(),
                        service: route.service_id.clone(),
                    })?;
                if gateway.exposure == Exposure::Public
                    && target.exposure == Exposure::Private
                    && !route.expose_private
                {
                    return Err(TopologyError::PublicGatewayExposesPrivateService {
                        gateway: gateway.id.clone(),
                        prefix: route.prefix.clone(),
                        service: target.id.clone(),
                    });
                }
            }
        }

        // Calls: declared endpoints, manifest-known permissions.
        for call in &self.calls {
            for endpoint in [&call.caller_id, &call.target_id] {
                if !service_ids.contains(endpoint.as_str()) {
                    return Err(TopologyError::UnknownCallService(endpoint.clone()));
                }
            }
            if !call.custom {
                let target = self
                    .services
                    .iter()
                    .find(|service| service.id == call.target_id)
                    .expect("target existence checked above");
                let known = target.manifest.permissions();
                for permission in &call.permissions {
                    if !known.contains(permission.as_str()) {
                        return Err(TopologyError::PermissionNotInTargetManifest {
                            caller: call.caller_id.clone(),
                            target: call.target_id.clone(),
                            permission: permission.clone(),
                        });
                    }
                }
            }
        }

        Ok(Topology {
            name: self.name,
            services: self.services,
            gateways: self.gateways,
            calls: self.calls,
        })
    }
}
