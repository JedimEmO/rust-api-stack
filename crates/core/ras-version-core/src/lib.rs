//! Core traits for versioned API migrations.

/// Converts one API version type into another.
///
/// Service macros use this trait for opt-in compatibility paths where a legacy
/// request is upgraded into the canonical request type, and the canonical
/// response is downgraded back into the legacy response type.
pub trait VersionMigration<From, To> {
    /// Error returned when a version migration cannot be performed.
    type Error: std::fmt::Display + Send + Sync + 'static;

    /// Convert `value` from one API version type into another.
    fn migrate(value: From) -> Result<To, Self::Error>;
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;
    use std::fmt;

    use super::*;

    #[derive(Debug, PartialEq)]
    struct LegacyUser {
        name: String,
    }

    #[derive(Debug, PartialEq)]
    struct CanonicalUser {
        display_name: String,
        active: bool,
    }

    #[derive(Debug, PartialEq)]
    struct LegacyResponse {
        name: String,
    }

    #[derive(Debug, PartialEq)]
    struct CanonicalResponse {
        display_name: String,
        active: bool,
    }

    #[derive(Debug, PartialEq)]
    struct MigrationError(&'static str);

    impl fmt::Display for MigrationError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str(self.0)
        }
    }

    struct UserMigration;

    struct IdentityMigration;

    impl VersionMigration<LegacyUser, CanonicalUser> for UserMigration {
        type Error = MigrationError;

        fn migrate(value: LegacyUser) -> Result<CanonicalUser, Self::Error> {
            if value.name.trim().is_empty() {
                return Err(MigrationError("name is required"));
            }

            Ok(CanonicalUser {
                display_name: value.name,
                active: true,
            })
        }
    }

    impl VersionMigration<CanonicalResponse, LegacyResponse> for UserMigration {
        type Error = MigrationError;

        fn migrate(value: CanonicalResponse) -> Result<LegacyResponse, Self::Error> {
            if !value.active {
                return Err(MigrationError("inactive users cannot be downgraded"));
            }

            Ok(LegacyResponse {
                name: value.display_name,
            })
        }
    }

    impl VersionMigration<String, String> for IdentityMigration {
        type Error = Infallible;

        fn migrate(value: String) -> Result<String, Self::Error> {
            Ok(value)
        }
    }

    fn assert_error_bounds<E: fmt::Display + Send + Sync + 'static>() {}

    #[test]
    fn migrate_returns_canonical_value_when_legacy_value_is_valid() {
        let legacy = LegacyUser {
            name: "Alice".to_string(),
        };

        let canonical = UserMigration::migrate(legacy).expect("migration succeeds");

        assert_eq!(
            canonical,
            CanonicalUser {
                display_name: "Alice".to_string(),
                active: true,
            }
        );
    }

    #[test]
    fn migrate_returns_domain_error_when_legacy_value_is_invalid() {
        let legacy = LegacyUser {
            name: " ".to_string(),
        };

        let error = UserMigration::migrate(legacy).expect_err("migration fails");

        assert_eq!(error.to_string(), "name is required");
    }

    #[test]
    fn same_migration_type_can_upgrade_requests_and_downgrade_responses() {
        let canonical = UserMigration::migrate(LegacyUser {
            name: "Alice".to_string(),
        })
        .expect("request upgrade succeeds");
        let legacy = UserMigration::migrate(CanonicalResponse {
            display_name: canonical.display_name,
            active: canonical.active,
        })
        .expect("response downgrade succeeds");

        assert_eq!(
            legacy,
            LegacyResponse {
                name: "Alice".to_string()
            }
        );
    }

    #[test]
    fn response_downgrade_can_return_domain_error() {
        let error = UserMigration::migrate(CanonicalResponse {
            display_name: "Alice".to_string(),
            active: false,
        })
        .expect_err("inactive user should not downgrade");

        assert_eq!(error.to_string(), "inactive users cannot be downgraded");
    }

    #[test]
    fn infallible_migration_can_use_standard_infallible_error_type() {
        let migrated = IdentityMigration::migrate("unchanged".to_string())
            .expect("infallible migration succeeds");

        assert_eq!(migrated, "unchanged");
    }

    #[test]
    fn migration_error_type_satisfies_public_trait_bounds() {
        assert_error_bounds::<MigrationError>();
        assert_error_bounds::<Infallible>();
    }

    #[test]
    fn migration_trait_can_be_called_through_generic_helper() {
        fn migrate_with<M, From, To>(value: From) -> Result<To, M::Error>
        where
            M: VersionMigration<From, To>,
        {
            M::migrate(value)
        }

        let canonical: CanonicalUser = migrate_with::<UserMigration, _, _>(LegacyUser {
            name: "Alice".to_string(),
        })
        .expect("generic migration succeeds");

        assert_eq!(
            canonical,
            CanonicalUser {
                display_name: "Alice".to_string(),
                active: true,
            }
        );
    }
}
