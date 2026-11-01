//! Data types that describe which permissions each operation of a RAS service
//! requires.
//!
//! With their `permissions` feature, the service macros (`ras-rest-macro`,
//! `ras-jsonrpc-macro`, `ras-file-macro` and `ras-jsonrpc-bidirectional-macro`)
//! generate values of these types from the service definition:
//!
//! - `generate_<service>_permission_manifest()` returns a
//!   [`ServicePermissions`].
//! - `<service>_permissions::<PERMISSION>` is a [`PermissionRef`] per
//!   permission string.
//! - `<service>_permissions::operations::<OPERATION>` is a
//!   [`StaticPermissionRequirement`] per protected operation.
//!
//! A build script can combine services into a [`PermissionManifest`] and write
//! it with [`write_manifest`] as an audit or tooling artifact. Token issuing
//! code can collect the generated constants in a [`PermissionSet`] instead of
//! spelling permission strings by hand.
//!
//! The crate has no transport code and no Cargo features. See the
//! [permission manifests chapter](https://github.com/JedimEmO/rust-api-stack/blob/master/documentation/src/permission-manifests.md).
//!
//! # Permission groups
//!
//! A requirement is a list of groups. The caller must hold every permission
//! of at least one group. A requirement with no groups, or only empty groups,
//! admits any authenticated caller. An empty group next to non-empty groups
//! is ignored, so `["admin"] | []` still requires `admin`. These are the
//! rules `ras-auth-core` enforces on the server, and the types here give the
//! same answer.

#![warn(missing_docs)]

use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashSet};
use std::path::Path;

/// The manifest format version that [`PermissionManifest::from_services`]
/// writes.
///
/// Tooling should compare it before reading the rest of a manifest: a reader
/// built for an older version can fail on a variant it does not know, such as
/// [`AuthRequirementInfo::Optional`].
pub const SCHEMA_VERSION: u32 = 2;

/// A combined permission manifest for one or more services.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionManifest {
    /// The manifest format version; [`SCHEMA_VERSION`] for manifests this
    /// crate builds.
    pub schema_version: u32,
    /// The services, sorted by name and then transport by
    /// [`PermissionManifest::from_services`].
    pub services: Vec<ServicePermissions>,
}

impl PermissionManifest {
    /// Builds a manifest from service-level permission metadata.
    ///
    /// Sorts the services by name and transport, and the operations of each
    /// service by [`ServicePermissions::sort_operations`], so the same input
    /// always serializes to the same JSON whatever order it arrives in.
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

    /// Returns every permission string the manifest's operations require,
    /// sorted and without duplicates.
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

/// Writes a permission manifest to `path` as pretty-printed JSON.
///
/// Creates missing parent directories and replaces an existing file.
///
/// # Errors
///
/// Returns the I/O error when a directory or the file cannot be written.
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
    /// The service name from the macro definition.
    pub service_name: String,
    /// The transport the service is served over.
    pub transport: TransportKind,
    /// One entry per callable operation, protected or public.
    pub operations: Vec<OperationPermissions>,
}

impl ServicePermissions {
    /// Sorts operations by `operation_id`, then `operation_name`.
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
    /// A REST service from `ras-rest-macro`.
    Rest,
    /// A JSON-RPC service from `ras-jsonrpc-macro`.
    JsonRpc,
    /// A file service from `ras-file-macro`.
    File,
    /// A bidirectional JSON-RPC service from
    /// `ras-jsonrpc-bidirectional-macro`.
    JsonRpcBidirectional,
}

/// Operation kind within a service transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    /// A REST endpoint.
    RestEndpoint,
    /// A JSON-RPC method.
    JsonRpcMethod,
    /// A file upload endpoint.
    FileUpload,
    /// A file download endpoint.
    FileDownload,
    /// A method the client calls on the server over a bidirectional
    /// connection.
    BidirectionalClientToServer,
    /// A method the server calls on the client over a bidirectional
    /// connection.
    BidirectionalServerToClientCall,
}

/// Callable wire target for an operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WireTarget {
    /// An HTTP route of a REST service.
    Rest {
        /// The HTTP method, such as `GET`.
        method: String,
        /// The full route path, including the service base path.
        path: String,
    },
    /// A JSON-RPC method name.
    JsonRpc {
        /// The method name on the wire.
        method: String,
    },
    /// An HTTP route of a file service.
    File {
        /// The HTTP method, such as `POST`.
        method: String,
        /// The full route path, including the service base path.
        path: String,
    },
    /// A JSON-RPC method on a bidirectional connection.
    BidirectionalJsonRpc {
        /// Which side calls the method.
        direction: String,
        /// The method name on the wire.
        method: String,
    },
}

/// Permission metadata for one callable operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationPermissions {
    /// A stable identifier for the operation within its service.
    pub operation_id: String,
    /// The handler name from the macro definition.
    pub operation_name: String,
    /// What kind of operation this is.
    pub kind: OperationKind,
    /// How a client reaches the operation.
    pub wire: WireTarget,
    /// What a caller needs to be allowed to call it.
    pub auth: AuthRequirementInfo,
    /// The API version the operation belongs to, for versioned services.
    pub version: Option<String>,
    /// The `operation_id` of the unversioned operation this one is a version
    /// of, for versioned services.
    pub canonical_operation_id: Option<String>,
}

/// Effective auth requirement for an operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthRequirementInfo {
    /// A public route (`UNAUTHORIZED`); no credential is read.
    Public,
    /// Public route that opportunistically identifies its caller (`OPTIONAL_AUTH`):
    /// anonymous callers are allowed, but a valid credential is authenticated and
    /// surfaced to the handler. No permission is required.
    Optional,
    /// Any authenticated caller is allowed; no permission is required.
    Authenticated,
    /// The caller must hold every permission of at least one group.
    Permissions {
        /// The groups, each non-empty, sorted and without duplicates.
        any_of: Vec<PermissionGroupInfo>,
    },
}

impl AuthRequirementInfo {
    /// Builds the effective auth requirement from permission groups.
    ///
    /// Groups are ORed together, and each group's permissions are ANDed.
    /// Returns [`AuthRequirementInfo::Authenticated`] when there are no
    /// groups or every group is empty. Otherwise empty groups are dropped,
    /// so `["admin"] | []` yields a requirement for `admin`, matching what
    /// `ras-auth-core` enforces. Permissions within a group and the groups
    /// themselves are sorted and deduplicated.
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
            if !all_of.is_empty() {
                any_of.push(PermissionGroupInfo { all_of });
            }
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
    /// The permissions the caller must all hold, sorted and without
    /// duplicates.
    pub all_of: Vec<String>,
}

/// A generated, typo-safe reference to a permission string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PermissionRef {
    name: &'static str,
}

impl PermissionRef {
    /// Wraps a permission string. The service macros call this; hand-written
    /// code should use the generated constants instead.
    pub const fn new(name: &'static str) -> Self {
        Self { name }
    }

    /// Returns the permission string.
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
///
/// Follows the rules in [Permission groups](crate#permission-groups): with
/// no groups or only empty groups any authenticated caller passes; otherwise
/// empty groups are ignored and the caller must satisfy a non-empty one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StaticPermissionRequirement {
    /// The groups as written in the service definition: ORed together, each
    /// one ANDed.
    pub any_of: &'static [&'static [&'static str]],
}

impl StaticPermissionRequirement {
    /// Wraps permission groups. The service macros call this; hand-written
    /// code should use the generated constants instead.
    pub const fn new(any_of: &'static [&'static [&'static str]]) -> Self {
        Self { any_of }
    }

    /// Returns a requirement any authenticated caller meets.
    pub const fn authenticated_only() -> Self {
        Self { any_of: &[] }
    }

    /// Returns `true` when any authenticated caller meets the requirement,
    /// which is when there are no groups or every group is empty.
    pub const fn is_authenticated_only(self) -> bool {
        let mut index = 0;
        while index < self.any_of.len() {
            if !self.any_of[index].is_empty() {
                return false;
            }
            index += 1;
        }
        true
    }

    /// Returns `true` when `permissions` holds every permission of at least
    /// one non-empty group.
    ///
    /// Always `true` for an authenticated-only requirement. An empty group
    /// next to non-empty groups never satisfies the requirement on its own.
    pub fn is_satisfied_by(self, permissions: &HashSet<String>) -> bool {
        self.is_authenticated_only()
            || self.non_empty_groups().any(|group| {
                group
                    .iter()
                    .all(|permission| permissions.contains(*permission))
            })
    }

    /// Returns the permissions of the first non-empty group, which are
    /// enough to satisfy the requirement. Empty for an authenticated-only
    /// requirement.
    pub fn first_group_permissions(self) -> impl Iterator<Item = PermissionRef> {
        self.non_empty_groups()
            .next()
            .unwrap_or(&[])
            .iter()
            .copied()
            .map(PermissionRef::new)
    }

    fn non_empty_groups(self) -> impl Iterator<Item = &'static [&'static str]> {
        self.any_of
            .iter()
            .copied()
            .filter(|group| !group.is_empty())
    }
}

/// Builder for token/session permission claims using generated constants.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PermissionSet {
    permissions: BTreeSet<&'static str>,
}

impl PermissionSet {
    /// Returns an empty set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the set with `permission` added.
    pub fn with(mut self, permission: PermissionRef) -> Self {
        self.permissions.insert(permission.as_str());
        self
    }

    /// Adds `permission`; adding one twice has no effect.
    pub fn insert(&mut self, permission: PermissionRef) {
        self.permissions.insert(permission.as_str());
    }

    /// Adds every permission of the requirement's first non-empty group,
    /// which is enough to satisfy it. Adds nothing for an authenticated-only
    /// requirement.
    pub fn extend_first_group(&mut self, requirement: StaticPermissionRequirement) {
        self.permissions.extend(
            requirement
                .first_group_permissions()
                .map(PermissionRef::as_str),
        );
    }

    /// Returns the permission strings, in the form `AuthenticatedUser` and
    /// session claims carry them.
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
    fn only_empty_permission_groups_are_authenticated_only() {
        let none = AuthRequirementInfo::from_permission_groups(Vec::<Vec<&str>>::new());
        let all_empty =
            AuthRequirementInfo::from_permission_groups([Vec::<&str>::new(), Vec::new()]);

        assert_eq!(none, AuthRequirementInfo::Authenticated);
        assert_eq!(all_empty, AuthRequirementInfo::Authenticated);
    }

    #[test]
    fn empty_group_next_to_non_empty_groups_is_ignored() {
        let auth = AuthRequirementInfo::from_permission_groups([vec!["admin"], vec![]]);

        assert_eq!(
            auth,
            AuthRequirementInfo::Permissions {
                any_of: vec![PermissionGroupInfo {
                    all_of: vec!["admin".to_string()],
                }],
            }
        );
    }

    fn perms(permissions: &[&str]) -> HashSet<String> {
        permissions.iter().map(|p| p.to_string()).collect()
    }

    #[test]
    fn static_requirement_with_only_empty_groups_admits_any_user() {
        for requirement in [
            StaticPermissionRequirement::authenticated_only(),
            StaticPermissionRequirement::new(&[&[]]),
            StaticPermissionRequirement::new(&[&[], &[]]),
        ] {
            assert!(requirement.is_authenticated_only());
            assert!(requirement.is_satisfied_by(&perms(&[])));
            assert_eq!(requirement.first_group_permissions().count(), 0);
        }
    }

    #[test]
    fn static_requirement_ignores_empty_group_next_to_non_empty_groups() {
        const REQUIREMENT: StaticPermissionRequirement =
            StaticPermissionRequirement::new(&[&[], &["admin"]]);

        assert!(!REQUIREMENT.is_authenticated_only());
        assert!(!REQUIREMENT.is_satisfied_by(&perms(&[])));
        assert!(!REQUIREMENT.is_satisfied_by(&perms(&["items:read"])));
        assert!(REQUIREMENT.is_satisfied_by(&perms(&["admin"])));
        assert_eq!(
            REQUIREMENT.first_group_permissions().collect::<Vec<_>>(),
            vec![PermissionRef::new("admin")]
        );

        let mut set = PermissionSet::new();
        set.extend_first_group(REQUIREMENT);
        assert!(REQUIREMENT.is_satisfied_by(&set.into_hash_set()));
    }

    #[test]
    fn static_requirement_needs_every_permission_of_one_group() {
        const REQUIREMENT: StaticPermissionRequirement =
            StaticPermissionRequirement::new(&[&["items:read", "items:list"], &["admin"]]);

        assert!(!REQUIREMENT.is_authenticated_only());
        assert!(!REQUIREMENT.is_satisfied_by(&perms(&[])));
        assert!(!REQUIREMENT.is_satisfied_by(&perms(&["items:read"])));
        assert!(REQUIREMENT.is_satisfied_by(&perms(&["items:read", "items:list"])));
        assert!(REQUIREMENT.is_satisfied_by(&perms(&["admin"])));
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
