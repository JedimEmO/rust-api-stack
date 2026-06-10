//! Topology validation errors.

use thiserror::Error;

/// Validation failures raised by [`crate::TopologyBuilder::build`].
///
/// All topology validation is deterministic build/test-time validation:
/// run `build()` in a test (or build script) and the graph is checked on
/// every compile of that test target.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TopologyError {
    #[error("duplicate service id {0:?}")]
    DuplicateServiceId(String),

    #[error("duplicate audience {audience:?} (services {first:?} and {second:?})")]
    DuplicateAudience {
        audience: String,
        first: String,
        second: String,
    },

    #[error("duplicate gateway id {0:?}")]
    DuplicateGatewayId(String),

    #[error("gateway {gateway:?} declares duplicate route prefix {prefix:?}")]
    DuplicateRoutePrefix { gateway: String, prefix: String },

    #[error("gateway {gateway:?} route {prefix:?} targets undeclared service {service:?}")]
    UnknownRouteTarget {
        gateway: String,
        prefix: String,
        service: String,
    },

    #[error(
        "public gateway {gateway:?} exposes private service {service:?} via {prefix:?}; \
         mark the route expose_private to allow this deliberately"
    )]
    PublicGatewayExposesPrivateService {
        gateway: String,
        prefix: String,
        service: String,
    },

    #[error("call edge references undeclared service {0:?}")]
    UnknownCallService(String),

    #[error(
        "call {caller:?} -> {target:?} uses permission {permission:?} which is not in \
         {target:?}'s imported manifest; use call_with_custom_permissions if intentional"
    )]
    PermissionNotInTargetManifest {
        caller: String,
        target: String,
        permission: String,
    },

    #[error("unknown gateway {0:?}")]
    UnknownGateway(String),

    #[error("invalid topology: {0}")]
    Invalid(String),
}
