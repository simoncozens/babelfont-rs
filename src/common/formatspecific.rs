use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
/// A map for storing format-specific data.
// Don't typeshare this; use type aliases instead
pub struct FormatSpecific(IndexMap<String, Value>);

impl FormatSpecific {
    /// Get a value by key
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.get(key)
    }

    /// Insert a key-value pair
    pub fn insert(&mut self, key: String, value: Value) {
        self.0.insert(key, value);
    }

    /// Check if the map contains a key
    pub fn contains_key(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }

    /// Get a string value by key, or an empty string if not found
    pub(crate) fn get_string(&self, key: &str) -> String {
        self.0
            .get(key)
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string()
    }

    pub(crate) fn get_optionstring(&self, key: &str) -> Option<String> {
        self.0
            .get(key)
            .and_then(|x| x.as_str())
            .map(|x| x.to_string())
    }

    pub(crate) fn get_bool(&self, key: &str) -> bool {
        self.0
            .get(key)
            .and_then(|x| x.as_bool())
            .unwrap_or_default()
    }

    /// Get an iterator over the key-value pairs
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.0.iter()
    }

    /// Check if the FormatSpecific is empty
    pub fn is_empty(value: &FormatSpecific) -> bool {
        value.0.is_empty()
    }
}
