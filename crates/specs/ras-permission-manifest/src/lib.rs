//! Permission manifest types for Rust Agent Stack service definitions.
//!
//! This crate is intentionally transport-free. Service macro crates generate
//! values of these types, and build scripts can serialize them as an audit or
//! tooling artifact.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashSet};
use std::path::Path;

/// Current permission manifest schema version.
pub const SCHEMA_VERSION: u32 = 1;

/// A combined permission manifest for one or more services.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionManifest {
    pub schema_version: u32,
    pub services: Vec<ServicePermissions>,
}

impl PermissionManifest {
    /// Build a deterministic manifest from service-level permission metadata.
    pub fn from_services<I>(services: I) -> Self
    where
        I: IntoIterator<Item = ServicePermissions>,
    {
        let mut services: Vec<_> = services.into_iter().collect();
        for service in &mut services {
            service.sort_operations();
        }
        services.sort_by(|left, right| {
            left.service_name
                .cmp(&right.service_name)
                .then_with(|| left.transport.cmp(&right.transport))
        });

        Self {
            schema_version: SCHEMA_VERSION,
            services,
        }
    }

    /// Return every permission string referenced by the manifest.
    pub fn permissions(&self) -> BTreeSet<&str> {
        let mut permissions = BTreeSet::new();
        for service in &self.services {
            for operation in &service.operations {
                if let AuthRequirementInfo::Permissions { any_of } = &operation.auth {
                    for group in any_of {
                        for permission in &group.all_of {
                            permissions.insert(permission.as_str());
                        }
                    }
                }
            }
        }
        permissions
    }
}

/// Write a permission manifest as pretty JSON.
pub fn write_manifest(
    path: impl AsRef<Path>,
    manifest: &PermissionManifest,
) -> std::io::Result<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(manifest).map_err(std::io::Error::other)?;
    std::fs::write(path, json)
}

/// Permission metadata for one generated service definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServicePermissions {
    pub service_name: String,
    pub transport: TransportKind,
    pub operations: Vec<OperationPermissions>,
}

impl ServicePermissions {
    pub fn sort_operations(&mut self) {
        self.operations.sort_by(|left, right| {
            left.operation_id
                .cmp(&right.operation_id)
                .then_with(|| left.operation_name.cmp(&right.operation_name))
        });
    }
}

/// Transport family for a generated service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportKind {
    Rest,
    JsonRpc,
    File,
    JsonRpcBidirectional,
}

/// Operation kind within a service transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    RestEndpoint,
    JsonRpcMethod,
    FileUpload,
    FileDownload,
    BidirectionalClientToServer,
    BidirectionalServerToClientCall,
}

/// Callable wire target for an operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WireTarget {
    Rest { method: String, path: String },
    JsonRpc { method: String },
    File { method: String, path: String },
    BidirectionalJsonRpc { direction: String, method: String },
}

/// Permission metadata for one callable operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationPermissions {
    pub operation_id: String,
    pub operation_name: String,
    pub kind: OperationKind,
    pub wire: WireTarget,
    pub auth: AuthRequirementInfo,
    pub version: Option<String>,
    pub canonical_operation_id: Option<String>,
}

/// Effective auth requirement for an operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthRequirementInfo {
    Public,
    Authenticated,
    Permissions { any_of: Vec<PermissionGroupInfo> },
}

impl AuthRequirementInfo {
    /// Build the effective auth requirement from permission groups.
    ///
    /// Groups are ORed together, and each group's permissions are ANDed. Any
    /// empty group makes the operation authenticated-only.
    pub fn from_permission_groups<I, G, P>(groups: I) -> Self
    where
        I: IntoIterator<Item = G>,
        G: IntoIterator<Item = P>,
        P: Into<String>,
    {
        let mut any_of = Vec::new();
        for group in groups {
            let mut all_of: Vec<String> = group.into_iter().map(Into::into).collect();
            all_of.sort();
            all_of.dedup();
            if all_of.is_empty() {
                return Self::Authenticated;
            }
            any_of.push(PermissionGroupInfo { all_of });
        }

        if any_of.is_empty() {
            Self::Authenticated
        } else {
            any_of.sort_by(|left, right| left.all_of.cmp(&right.all_of));
            any_of.dedup();
            Self::Permissions { any_of }
        }
    }
}

/// One AND group inside a permission requirement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionGroupInfo {
    pub all_of: Vec<String>,
}

/// A generated, typo-safe reference to a permission string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PermissionRef {
    name: &'static str,
}

impl PermissionRef {
    pub const fn new(name: &'static str) -> Self {
        Self { name }
    }

    pub const fn as_str(self) -> &'static str {
        self.name
    }
}

impl Serialize for PermissionRef {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.name)
    }
}

/// A generated static requirement for an operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StaticPermissionRequirement {
    pub any_of: &'static [&'static [&'static str]],
}

impl StaticPermissionRequirement {
    pub const fn new(any_of: &'static [&'static [&'static str]]) -> Self {
        Self { any_of }
    }

    pub const fn authenticated_only() -> Self {
        Self { any_of: &[] }
    }

    pub const fn is_authenticated_only(self) -> bool {
        self.any_of.is_empty()
    }

    pub fn is_satisfied_by(self, permissions: &HashSet<String>) -> bool {
        self.any_of.is_empty()
            || self.any_of.iter().any(|group| {
                group
                    .iter()
                    .all(|permission| permissions.contains(*permission))
            })
    }

    pub fn first_group_permissions(self) -> impl Iterator<Item = PermissionRef> {
        self.any_of
            .first()
            .copied()
            .unwrap_or(&[])
            .iter()
            .copied()
            .map(PermissionRef::new)
    }
}

/// Builder for token/session permission claims using generated constants.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PermissionSet {
    permissions: BTreeSet<&'static str>,
}

impl PermissionSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, permission: PermissionRef) -> Self {
        self.permissions.insert(permission.as_str());
        self
    }

    pub fn insert(&mut self, permission: PermissionRef) {
        self.permissions.insert(permission.as_str());
    }

    pub fn extend_first_group(&mut self, requirement: StaticPermissionRequirement) {
        self.permissions.extend(
            requirement
                .first_group_permissions()
                .map(PermissionRef::as_str),
        );
    }

    pub fn into_hash_set(self) -> HashSet<String> {
        self.permissions
            .into_iter()
            .map(ToOwned::to_owned)
            .collect()
    }
}

impl FromIterator<PermissionRef> for PermissionSet {
    fn from_iter<T: IntoIterator<Item = PermissionRef>>(iter: T) -> Self {
        let mut set = Self::new();
        for permission in iter {
            set.insert(permission);
        }
        set
    }
}

impl Extend<PermissionRef> for PermissionSet {
    fn extend<T: IntoIterator<Item = PermissionRef>>(&mut self, iter: T) {
        for permission in iter {
            self.insert(permission);
        }
    }
}

impl From<PermissionSet> for HashSet<String> {
    fn from(value: PermissionSet) -> Self {
        value.into_hash_set()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_groups_preserve_or_and_semantics() {
        let auth = AuthRequirementInfo::from_permission_groups([
            vec!["items:read", "items:list"],
            vec!["admin"],
        ]);

        assert_eq!(
            auth,
            AuthRequirementInfo::Permissions {
                any_of: vec![
                    PermissionGroupInfo {
                        all_of: vec!["admin".to_string()],
                    },
                    PermissionGroupInfo {
                        all_of: vec!["items:list".to_string(), "items:read".to_string()],
                    },
                ],
            }
        );
    }

    #[test]
    fn empty_permission_group_is_authenticated_only() {
        let auth = AuthRequirementInfo::from_permission_groups([Vec::<&str>::new()]);
        assert_eq!(auth, AuthRequirementInfo::Authenticated);
    }

    #[test]
    fn manifest_sorts_services_and_operations() {
        let mut left = ServicePermissions {
            service_name: "B".to_string(),
            transport: TransportKind::Rest,
            operations: vec![operation("z"), operation("a")],
        };
        let right = ServicePermissions {
            service_name: "A".to_string(),
            transport: TransportKind::JsonRpc,
            operations: vec![operation("m")],
        };
        left.sort_operations();

        let manifest = PermissionManifest::from_services([left, right]);
        assert_eq!(manifest.services[0].service_name, "A");
        assert_eq!(manifest.services[1].operations[0].operation_id, "a");
    }

    #[test]
    fn permission_set_builds_hash_set() {
        const READ: PermissionRef = PermissionRef::new("items:read");
        const WRITE: PermissionRef = PermissionRef::new("items:write");

        let permissions = PermissionSet::new()
            .with(READ)
            .with(WRITE)
            .with(READ)
            .into_hash_set();

        assert_eq!(permissions.len(), 2);
        assert!(permissions.contains("items:read"));
        assert!(permissions.contains("items:write"));
    }

    fn operation(operation_id: &str) -> OperationPermissions {
        OperationPermissions {
            operation_id: operation_id.to_string(),
            operation_name: operation_id.to_string(),
            kind: OperationKind::JsonRpcMethod,
            wire: WireTarget::JsonRpc {
                method: operation_id.to_string(),
            },
            auth: AuthRequirementInfo::Public,
            version: None,
            canonical_operation_id: None,
        }
    }
}
