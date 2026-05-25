//! Info Object and related types for OpenRPC specification.
//!
//! The Info object provides metadata about the API.

use crate::{Extensions, error::OpenRpcResult, validation::Validate};
use bon::Builder;
use serde::{Deserialize, Serialize};

/// The object provides metadata about the API.
/// The metadata MAY be used by the clients if needed, and MAY be presented
/// in editing or documentation generation tools for convenience.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Builder)]
#[serde(deny_unknown_fields)]
pub struct Info {
    /// The title of the application.
    pub title: String,

    /// A verbose description of the application.
    /// GitHub Flavored Markdown syntax MAY be used for rich text representation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// A URL to the Terms of Service for the API.
    /// MUST be in the format of a URL.
    #[serde(rename = "termsOfService", skip_serializing_if = "Option::is_none")]
    pub terms_of_service: Option<String>,

    /// The contact information for the exposed API.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contact: Option<Contact>,

    /// The license information for the exposed API.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<License>,

    /// The version of the OpenRPC document (which is distinct from the
    /// OpenRPC Specification version or the API implementation version).
    pub version: String,

    /// Specification extensions
    #[serde(flatten, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl Info {
    /// Create a new Info object with required fields
    pub fn new(title: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            description: None,
            terms_of_service: None,
            contact: None,
            license: None,
            version: version.into(),
            extensions: Extensions::new(),
        }
    }

    /// Set the description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the terms of service URL
    pub fn with_terms_of_service(mut self, terms_of_service: impl Into<String>) -> Self {
        self.terms_of_service = Some(terms_of_service.into());
        self
    }

    /// Set the contact information
    pub fn with_contact(mut self, contact: Contact) -> Self {
        self.contact = Some(contact);
        self
    }

    /// Set the license information
    pub fn with_license(mut self, license: License) -> Self {
        self.license = Some(license);
        self
    }

    /// Add an extension field
    pub fn with_extension(
        mut self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.extensions.insert(key, value);
        self
    }
}

impl Validate for Info {
    fn validate(&self) -> OpenRpcResult<()> {
        // Validate title
        if self.title.is_empty() {
            return Err(crate::error::OpenRpcError::missing_field("title"));
        }

        // Validate version
        if self.version.is_empty() {
            return Err(crate::error::OpenRpcError::missing_field("version"));
        }
        crate::validation::validate_semver(&self.version)?;

        // Validate terms of service URL if present
        if let Some(ref tos) = self.terms_of_service {
            crate::validation::validate_url(tos)?;
        }

        // Validate contact if present
        if let Some(ref contact) = self.contact {
            contact.validate()?;
        }

        // Validate license if present
        if let Some(ref license) = self.license {
            license.validate()?;
        }

        // Validate extensions
        self.extensions.validate()?;

        Ok(())
    }
}

/// Contact information for the exposed API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Builder)]
#[serde(deny_unknown_fields)]
pub struct Contact {
    /// The identifying name of the contact person/organization.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// The URL pointing to the contact information.
    /// MUST be in the format of a URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    /// The email address of the contact person/organization.
    /// MUST be in the format of an email address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,

    /// Specification extensions
    #[serde(flatten, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl Contact {
    /// Create a new empty Contact
    pub fn new() -> Self {
        Self {
            name: None,
            url: None,
            email: None,
            extensions: Extensions::new(),
        }
    }

    /// Set the contact name
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Set the contact URL
    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    /// Set the contact email
    pub fn with_email(mut self, email: impl Into<String>) -> Self {
        self.email = Some(email.into());
        self
    }

    /// Add an extension field
    pub fn with_extension(
        mut self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.extensions.insert(key, value);
        self
    }
}

impl Default for Contact {
    fn default() -> Self {
        Self::new()
    }
}

impl Validate for Contact {
    fn validate(&self) -> OpenRpcResult<()> {
        // Validate URL if present
        if let Some(ref url) = self.url {
            crate::validation::validate_url(url)?;
        }

        // Validate email if present
        if let Some(ref email) = self.email {
            crate::validation::validate_email(email)?;
        }

        // Validate extensions
        self.extensions.validate()?;

        Ok(())
    }
}

/// License information for the exposed API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Builder)]
#[serde(deny_unknown_fields)]
pub struct License {
    /// The license name used for the API.
    pub name: String,

    /// A URL to the license used for the API.
    /// MUST be in the format of a URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    /// Specification extensions
    #[serde(flatten, skip_serializing_if = "Extensions::is_empty")]
    #[builder(default)]
    pub extensions: Extensions,
}

impl License {
    /// Create a new License with required name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            url: None,
            extensions: Extensions::new(),
        }
    }

    /// Set the license URL
    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    /// Add an extension field
    pub fn with_extension(
        mut self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.extensions.insert(key, value);
        self
    }
}

impl Validate for License {
    fn validate(&self) -> OpenRpcResult<()> {
        // Validate name
        if self.name.is_empty() {
            return Err(crate::error::OpenRpcError::missing_field("name"));
        }

        // Validate URL if present
        if let Some(ref url) = self.url {
            crate::validation::validate_url(url)?;
        }

        // Validate extensions
        self.extensions.validate()?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_info_creation() {
        let info = Info::new("Test API", "1.0.0");
        assert_eq!(info.title, "Test API");
        assert_eq!(info.version, "1.0.0");
        assert!(info.description.is_none());
    }

    #[test]
    fn test_info_builder() {
        let info = Info::builder()
            .title("Test API".to_string())
            .version("1.0.0".to_string())
            .description("A test API".to_string())
            .contact(Contact::new().with_email("test@example.com"))
            .license(License::new("MIT"))
            .build();

        assert_eq!(info.title, "Test API");
        assert_eq!(info.description, Some("A test API".to_string()));
        assert!(info.contact.is_some());
        assert!(info.license.is_some());
    }

    #[test]
    fn test_info_validation() {
        // Valid info
        let info = Info::new("Test API", "1.0.0");
        assert!(info.validate().is_ok());

        // Invalid - empty title
        let info = Info::new("", "1.0.0");
        assert!(info.validate().is_err());

        // Invalid - empty version
        let info = Info::new("Test", "");
        assert!(info.validate().is_err());

        // Invalid - bad terms of service URL
        let info = Info::new("Test", "1.0.0").with_terms_of_service("not-a-url");
        assert!(info.validate().is_err());
    }

    #[test]
    fn test_contact_creation() {
        let contact = Contact::new()
            .with_name("John Doe")
            .with_email("john@example.com")
            .with_url("https://johndoe.com");

        assert_eq!(contact.name, Some("John Doe".to_string()));
        assert_eq!(contact.email, Some("john@example.com".to_string()));
        assert_eq!(contact.url, Some("https://johndoe.com".to_string()));
    }

    #[test]
    fn test_contact_validation() {
        // Valid contact
        let contact = Contact::new().with_email("test@example.com");
        assert!(contact.validate().is_ok());

        // Invalid email
        let contact = Contact::new().with_email("invalid-email");
        assert!(contact.validate().is_err());

        // Invalid URL
        let contact = Contact::new().with_url("not-a-url");
        assert!(contact.validate().is_err());
    }

    #[test]
    fn test_license_creation() {
        let license = License::new("MIT").with_url("https://opensource.org/licenses/MIT");

        assert_eq!(license.name, "MIT");
        assert_eq!(
            license.url,
            Some("https://opensource.org/licenses/MIT".to_string())
        );
    }

    #[test]
    fn test_license_validation() {
        // Valid license
        let license = License::new("MIT");
        assert!(license.validate().is_ok());

        // Invalid - empty name
        let license = License::new("");
        assert!(license.validate().is_err());

        // Invalid URL
        let license = License::new("MIT").with_url("not-a-url");
        assert!(license.validate().is_err());
    }

    #[test]
    fn test_info_serialization() {
        let info = Info::builder()
            .title("Test API".to_string())
            .version("1.0.0".to_string())
            .description("A test API".to_string())
            .build();

        let json = serde_json::to_value(&info).unwrap();
        let expected = json!({
            "title": "Test API",
            "version": "1.0.0",
            "description": "A test API"
        });

        assert_eq!(json, expected);

        let deserialized: Info = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized, info);
    }

    #[test]
    fn test_info_with_extensions() {
        let info = Info::new("Test", "1.0.0").with_extension("x-custom", "value");

        assert!(!info.extensions.is_empty());
        assert_eq!(info.extensions.get("x-custom"), Some(&json!("value")));

        let json = serde_json::to_value(&info).unwrap();
        assert!(json.as_object().unwrap().contains_key("x-custom"));
    }

    #[test]
    fn test_contact_builder() {
        let contact = Contact::builder()
            .name("John Doe".to_string())
            .email("john@example.com".to_string())
            .build();

        assert_eq!(contact.name, Some("John Doe".to_string()));
        assert_eq!(contact.email, Some("john@example.com".to_string()));
    }

    #[test]
    fn test_license_builder() {
        let license = License::builder()
            .name("MIT".to_string())
            .url("https://opensource.org/licenses/MIT".to_string())
            .build();

        assert_eq!(license.name, "MIT");
        assert_eq!(
            license.url,
            Some("https://opensource.org/licenses/MIT".to_string())
        );
    }
}
