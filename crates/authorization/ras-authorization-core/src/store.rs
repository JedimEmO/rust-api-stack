//! Authorization storage: the read trait used by the issuer plus an
//! in-memory implementation with the embedded-mode management API.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use async_trait::async_trait;
use ras_permission_manifest::PermissionManifest;
use tokio::sync::RwLock;

use crate::error::AuthzError;
use crate::model::{
    AudiencePermission, Principal, ResolvedPermissions, RoleDefinition, ServiceRegistration,
};

/// Read interface the token issuer needs. Production deployments implement
/// this over their database; [`InMemoryAuthorizationStore`] serves embedded
/// mode, tests, and examples.
#[async_trait]
pub trait AuthorizationStore: Send + Sync {
    async fn get_service(
        &self,
        service_id: &str,
    ) -> Result<Option<ServiceRegistration>, AuthzError>;

    /// Whether any registered service owns `audience`.
    async fn audience_exists(&self, audience: &str) -> Result<bool, AuthzError>;

    /// All permissions the principal holds, grouped by audience (direct
    /// grants plus role bindings).
    async fn resolve_permissions(
        &self,
        principal: &Principal,
    ) -> Result<ResolvedPermissions, AuthzError>;

    /// Monotonic version, bumped on every authorization mutation. Stamped
    /// into issued tokens as `authz_version`.
    async fn authz_version(&self) -> Result<u64, AuthzError>;
}

#[derive(Default)]
struct StoreState {
    services: HashMap<String, ServiceRegistration>,
    /// audience -> permissions known from imported manifests.
    known_permissions: HashMap<String, BTreeSet<String>>,
    roles: HashMap<String, RoleDefinition>,
    role_bindings: HashMap<Principal, BTreeSet<String>>,
    direct_grants: HashMap<Principal, BTreeSet<AudiencePermission>>,
    version: u64,
}

impl StoreState {
    fn bump(&mut self) {
        self.version += 1;
    }

    fn check_known(&self, grant: &AudiencePermission) -> Result<(), AuthzError> {
        let known = self
            .known_permissions
            .get(&grant.audience)
            .is_some_and(|permissions| permissions.contains(&grant.permission));
        if known {
            Ok(())
        } else {
            Err(AuthzError::UnknownPermission {
                audience: grant.audience.clone(),
                permission: grant.permission.clone(),
            })
        }
    }
}

/// In-memory authorization store with the embedded-mode management API.
///
/// Grants default to manifest-known permissions only: import each service's
/// generated permission manifest, then grant. Permissions outside any
/// imported manifest require the explicit `*_custom` methods, keeping
/// ad-hoc strings visible at the call site.
#[derive(Default)]
pub struct InMemoryAuthorizationStore {
    state: RwLock<StoreState>,
}

impl InMemoryAuthorizationStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register (or replace) a service. The audience must be unique across
    /// registered services.
    pub async fn register_service(
        &self,
        registration: ServiceRegistration,
    ) -> Result<(), AuthzError> {
        if registration.service_id.is_empty() || registration.audience.is_empty() {
            return Err(AuthzError::InvalidConfig(
                "service_id and audience must be non-empty".to_string(),
            ));
        }
        let mut state = self.state.write().await;
        let audience_taken = state.services.values().any(|service| {
            service.audience == registration.audience
                && service.service_id != registration.service_id
        });
        if audience_taken {
            return Err(AuthzError::InvalidConfig(format!(
                "audience {:?} is already registered to another service",
                registration.audience
            )));
        }
        state
            .services
            .insert(registration.service_id.clone(), registration);
        state.bump();
        Ok(())
    }

    /// Disable a service: it can no longer be issued tokens.
    pub async fn set_service_enabled(
        &self,
        service_id: &str,
        enabled: bool,
    ) -> Result<(), AuthzError> {
        let mut state = self.state.write().await;
        let service =
            state
                .services
                .get_mut(service_id)
                .ok_or_else(|| AuthzError::UnknownService {
                    service_id: service_id.to_string(),
                })?;
        service.enabled = enabled;
        state.bump();
        Ok(())
    }

    /// Import a generated permission manifest as the known-permission
    /// vocabulary for `audience`. Subsequent grants for that audience are
    /// validated against it.
    pub async fn import_manifest(
        &self,
        audience: impl Into<String>,
        manifest: &PermissionManifest,
    ) -> Result<usize, AuthzError> {
        let audience = audience.into();
        let permissions: BTreeSet<String> = manifest
            .permissions()
            .into_iter()
            .map(str::to_string)
            .collect();
        let count = permissions.len();
        let mut state = self.state.write().await;
        state
            .known_permissions
            .entry(audience)
            .or_default()
            .extend(permissions);
        state.bump();
        Ok(count)
    }

    /// Define (or replace) a role. Every permission must be known from an
    /// imported manifest.
    pub async fn define_role(&self, role: RoleDefinition) -> Result<(), AuthzError> {
        let mut state = self.state.write().await;
        for grant in &role.permissions {
            state.check_known(grant)?;
        }
        state.roles.insert(role.role_id.clone(), role);
        state.bump();
        Ok(())
    }

    /// Bind a role to a principal.
    pub async fn bind_role(
        &self,
        principal: Principal,
        role_id: impl Into<String>,
    ) -> Result<(), AuthzError> {
        let role_id = role_id.into();
        let mut state = self.state.write().await;
        if !state.roles.contains_key(&role_id) {
            return Err(AuthzError::InvalidConfig(format!(
                "role {role_id:?} is not defined"
            )));
        }
        state
            .role_bindings
            .entry(principal)
            .or_default()
            .insert(role_id);
        state.bump();
        Ok(())
    }

    /// Grant a manifest-known permission directly to a principal.
    pub async fn grant(
        &self,
        principal: Principal,
        grant: AudiencePermission,
    ) -> Result<(), AuthzError> {
        let mut state = self.state.write().await;
        state.check_known(&grant)?;
        state
            .direct_grants
            .entry(principal)
            .or_default()
            .insert(grant);
        state.bump();
        Ok(())
    }

    /// Grant a permission that is *not* part of any imported manifest.
    /// Explicitly named so custom/manual permissions stay visible.
    pub async fn grant_custom(
        &self,
        principal: Principal,
        grant: AudiencePermission,
    ) -> Result<(), AuthzError> {
        let mut state = self.state.write().await;
        state
            .direct_grants
            .entry(principal)
            .or_default()
            .insert(grant);
        state.bump();
        Ok(())
    }

    /// Revoke a direct grant. Returns whether it existed.
    pub async fn revoke(
        &self,
        principal: &Principal,
        grant: &AudiencePermission,
    ) -> Result<bool, AuthzError> {
        let mut state = self.state.write().await;
        let removed = state
            .direct_grants
            .get_mut(principal)
            .is_some_and(|grants| grants.remove(grant));
        if removed {
            state.bump();
        }
        Ok(removed)
    }

    /// Remove a role binding. Returns whether it existed.
    pub async fn unbind_role(
        &self,
        principal: &Principal,
        role_id: &str,
    ) -> Result<bool, AuthzError> {
        let mut state = self.state.write().await;
        let removed = state
            .role_bindings
            .get_mut(principal)
            .is_some_and(|roles| roles.remove(role_id));
        if removed {
            state.bump();
        }
        Ok(removed)
    }
}

#[async_trait]
impl AuthorizationStore for InMemoryAuthorizationStore {
    async fn get_service(
        &self,
        service_id: &str,
    ) -> Result<Option<ServiceRegistration>, AuthzError> {
        Ok(self.state.read().await.services.get(service_id).cloned())
    }

    async fn audience_exists(&self, audience: &str) -> Result<bool, AuthzError> {
        Ok(self
            .state
            .read()
            .await
            .services
            .values()
            .any(|service| service.audience == audience))
    }

    async fn resolve_permissions(
        &self,
        principal: &Principal,
    ) -> Result<ResolvedPermissions, AuthzError> {
        let state = self.state.read().await;
        let mut resolved: ResolvedPermissions = BTreeMap::new();

        let mut add = |grant: &AudiencePermission| {
            resolved
                .entry(grant.audience.clone())
                .or_default()
                .insert(grant.permission.clone());
        };

        if let Some(grants) = state.direct_grants.get(principal) {
            for grant in grants {
                add(grant);
            }
        }
        if let Some(roles) = state.role_bindings.get(principal) {
            for role_id in roles {
                if let Some(role) = state.roles.get(role_id) {
                    for grant in &role.permissions {
                        add(grant);
                    }
                }
            }
        }
        Ok(resolved)
    }

    async fn authz_version(&self) -> Result<u64, AuthzError> {
        Ok(self.state.read().await.version)
    }
}
