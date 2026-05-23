//! Specification Extensions support for OpenRPC.
//!
//! While the OpenRPC Specification tries to accommodate most use cases,
//! additional data can be added to extend the specification at certain points.

use crate::error::{OpenRpcError, OpenRpcResult};
use crate::validation::Validate;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// A map of extension fields that can be added to any OpenRPC object.
///
/// Extensions are patterned fields that are always prefixed by "x-".
/// The field name MUST begin with x-, for example, x-internal-id.
/// The value can be null, a primitive, an array or an object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Extensions(HashMap<String, Value>);

impl Extensions {
    /// Create a new empty extensions map
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert an extension field.
    ///
    /// Extension keys must start with `x-`.
    pub fn insert(
        &mut self,
        key: impl Into<String>,
        value: impl Into<Value>,
    ) -> OpenRpcResult<&mut Self> {
        let key = key.into();
        validate_extension_key(&key)?;
        self.0.insert(key, value.into());
        Ok(self)
    }

    /// Get an extension field value
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.get(key)
    }

    /// Remove an extension field
    pub fn remove(&mut self, key: &str) -> Option<Value> {
        self.0.remove(key)
    }

    /// Check if an extension field exists
    pub fn contains_key(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }

    /// Get all extension keys
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.0.keys()
    }

    /// Get all extension values
    pub fn values(&self) -> impl Iterator<Item = &Value> {
        self.0.values()
    }

    /// Iterate over all extension key-value pairs
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.0.iter()
    }

    /// Check if extensions map is empty
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Get the number of extensions
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Merge another extensions map into this one
    pub fn merge(&mut self, other: Extensions) {
        self.0.extend(other.0);
    }

    /// Create an Extensions map from a HashMap
    pub fn from_map(map: HashMap<String, Value>) -> OpenRpcResult<Self> {
        for key in map.keys() {
            validate_extension_key(key)?;
        }
        Ok(Self(map))
    }

    /// Convert to a HashMap
    pub fn into_map(self) -> HashMap<String, Value> {
        self.0
    }

    /// Builder pattern for adding extensions
    pub fn with(mut self, key: impl Into<String>, value: impl Into<Value>) -> OpenRpcResult<Self> {
        self.insert(key, value)?;
        Ok(self)
    }
}

fn validate_extension_key(key: &str) -> OpenRpcResult<()> {
    if key.starts_with("x-") {
        Ok(())
    } else {
        Err(OpenRpcError::validation(format!(
            "Extension key must start with 'x-': {key}"
        )))
    }
}

impl Validate for Extensions {
    fn validate(&self) -> OpenRpcResult<()> {
        for key in self.0.keys() {
            if !key.starts_with("x-") {
                return Err(crate::error::OpenRpcError::validation(format!(
                    "Extension key must start with 'x-': {}",
                    key
                )));
            }

            if key.len() <= 2 {
                return Err(crate::error::OpenRpcError::validation(format!(
                    "Extension key must have content after 'x-': {}",
                    key
                )));
            }
        }
        Ok(())
    }
}

impl From<HashMap<String, Value>> for Extensions {
    fn from(map: HashMap<String, Value>) -> Self {
        Self(map)
    }
}

impl From<Extensions> for HashMap<String, Value> {
    fn from(extensions: Extensions) -> Self {
        extensions.0
    }
}

impl IntoIterator for Extensions {
    type Item = (String, Value);
    type IntoIter = std::collections::hash_map::IntoIter<String, Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a> IntoIterator for &'a Extensions {
    type Item = (&'a String, &'a Value);
    type IntoIter = std::collections::hash_map::Iter<'a, String, Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// Helper macro for creating extensions
#[macro_export]
macro_rules! extensions {
    () => {
        $crate::Extensions::new()
    };
    ($($key:expr => $value:expr),+ $(,)?) => {{
        let mut ext = $crate::Extensions::new();
        $(
            ext.insert($key, $value).expect("extension keys must start with 'x-'");
        )+
        ext
    }};
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_extensions_creation() {
        let mut ext = Extensions::new();
        ext.insert("x-custom", "value").unwrap();
        ext.insert("x-number", 42).unwrap();

        assert_eq!(
            ext.get("x-custom"),
            Some(&Value::String("value".to_string()))
        );
        assert_eq!(ext.get("x-number"), Some(&Value::Number(42.into())));
        assert_eq!(ext.len(), 2);
    }

    #[test]
    fn test_invalid_extension_key() {
        let mut ext = Extensions::new();
        let error = ext.insert("invalid-key", "value").unwrap_err();
        assert!(matches!(error, OpenRpcError::ValidationError { .. }));
        assert!(ext.is_empty());
    }

    #[test]
    fn test_extensions_validation() {
        let mut ext = Extensions::new();
        ext.insert("x-valid", "value").unwrap();
        assert!(ext.validate().is_ok());

        // Manually create invalid extension (bypassing insert validation)
        let invalid_map = HashMap::from([("invalid".to_string(), json!("value"))]);
        let invalid_ext = Extensions(invalid_map);
        assert!(invalid_ext.validate().is_err());

        // Test short extension key
        let short_map = HashMap::from([("x-".to_string(), json!("value"))]);
        let short_ext = Extensions(short_map);
        assert!(short_ext.validate().is_err());
    }

    #[test]
    fn test_extensions_merge() {
        let mut ext1 = Extensions::new();
        ext1.insert("x-first", "value1").unwrap();

        let mut ext2 = Extensions::new();
        ext2.insert("x-second", "value2").unwrap();

        ext1.merge(ext2);

        assert_eq!(ext1.len(), 2);
        assert!(ext1.contains_key("x-first"));
        assert!(ext1.contains_key("x-second"));
    }

    #[test]
    fn test_extensions_with_builder() {
        let ext = Extensions::new()
            .with("x-first", "value1")
            .unwrap()
            .with("x-second", 42)
            .unwrap();

        assert_eq!(ext.len(), 2);
        assert_eq!(
            ext.get("x-first"),
            Some(&Value::String("value1".to_string()))
        );
        assert_eq!(ext.get("x-second"), Some(&Value::Number(42.into())));
    }

    #[test]
    fn test_extensions_from_map() {
        let map = HashMap::from([
            ("x-custom".to_string(), json!("value")),
            ("x-number".to_string(), json!(42)),
        ]);

        let ext = Extensions::from_map(map.clone()).unwrap();
        assert_eq!(ext.len(), 2);

        // Test invalid map
        let invalid_map = HashMap::from([("invalid".to_string(), json!("value"))]);
        assert!(Extensions::from_map(invalid_map).is_err());
    }

    #[test]
    fn test_extensions_serialization() {
        let mut ext = Extensions::new();
        ext.insert("x-custom", "value").unwrap();
        ext.insert("x-number", 42).unwrap();

        let json = serde_json::to_value(&ext).unwrap();
        let expected = json!({
            "x-custom": "value",
            "x-number": 42
        });

        assert_eq!(json, expected);

        let deserialized: Extensions = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized, ext);
    }

    #[test]
    fn test_extensions_macro() {
        let ext = extensions![
            "x-first" => "value1",
            "x-second" => 42,
        ];

        assert_eq!(ext.len(), 2);
        assert_eq!(
            ext.get("x-first"),
            Some(&Value::String("value1".to_string()))
        );
        assert_eq!(ext.get("x-second"), Some(&Value::Number(42.into())));
    }

    #[test]
    fn test_extensions_iterator() {
        let mut ext = Extensions::new();
        ext.insert("x-first", "value1").unwrap();
        ext.insert("x-second", "value2").unwrap();

        let mut count = 0;
        for (key, _value) in &ext {
            assert!(key.starts_with("x-"));
            count += 1;
        }
        assert_eq!(count, 2);

        let collected: HashMap<String, Value> = ext.into_iter().collect();
        assert_eq!(collected.len(), 2);
    }
}
