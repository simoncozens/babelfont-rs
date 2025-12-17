use indexmap::IndexMap;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
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

    /// Insert any serializable value under the given key.
    #[inline]
    pub fn insert_json<K: Into<String>, T: Serialize>(&mut self, key: K, value: &T) {
        if let Ok(v) = serde_json::to_value(value) {
            self.insert(key.into(), v);
        }
    }

    /// Get any deserializable value under the given key, or return default.
    #[inline]
    pub fn get_json<K: Into<String>, T: DeserializeOwned + Default>(&self, key: K) -> T {
        self.0
            .get(&key.into())
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default()
    }

    /// Insert a serializable value only if it does not serialize to JSON null.
    #[inline]
    pub fn insert_json_non_null<K: Into<String>, T: Serialize>(&mut self, key: K, value: &T) {
        if let Ok(v) = serde_json::to_value(value) {
            if !v.is_null() {
                self.insert(key.into(), v);
            }
        }
    }

    /// Insert the inner value if `Some`, serialized under the given key.
    #[inline]
    pub fn insert_some_json<K: Into<String>, T: Serialize>(&mut self, key: K, value: &Option<T>) {
        if let Some(inner) = value.as_ref() {
            self.insert_json(key, inner);
        }
    }

    /// Insert when `value != default`, serialized under the given key.
    #[inline]
    pub fn insert_if_ne_json<
        K: Into<String>,
        T: Serialize + PartialEq<UT>,
        UT: ?Sized + PartialEq<T> + Serialize,
    >(
        &mut self,
        key: K,
        value: &T,
        default: &UT,
    ) {
        // Compare via PartialEq; if not equal, serialize the value.
        // We serialize `value` (not default) to store the non-default state.
        if value != default {
            self.insert_json(key, value);
        }
    }

    /// Insert when value implements `IsEmpty` and is not empty.
    #[inline]
    pub fn insert_nonempty_json<K: Into<String>, T: Serialize + IsEmpty>(
        &mut self,
        key: K,
        value: &T,
    ) {
        if !value.is_empty() {
            self.insert_json(key, value);
        }
    }

    /// Parse a typed value from JSON under `key`, returning `Option<T>`.
    #[inline]
    pub fn get_parse_opt<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.get(key)
            .cloned()
            .and_then(|v| serde_json::from_value::<T>(v).ok())
    }

    /// Parse a typed value from JSON under `key` or return `default`.
    #[inline]
    pub fn get_parse_or<T: DeserializeOwned>(&self, key: &str, default: T) -> T {
        self.get_parse_opt(key).unwrap_or(default)
    }

    /// Get a boolean by key or return `default`.
    #[inline]
    pub fn get_bool_or(&self, key: &str, default: bool) -> bool {
        self.get(key).and_then(|x| x.as_bool()).unwrap_or(default)
    }
}

/// Trait for checking emptiness across common container/string types.
pub trait IsEmpty {
    fn is_empty(&self) -> bool;
}

impl IsEmpty for String {
    #[inline]
    fn is_empty(&self) -> bool {
        String::is_empty(self)
    }
}
impl IsEmpty for &str {
    #[inline]
    fn is_empty(&self) -> bool {
        (**self).is_empty()
    }
}
impl<T> IsEmpty for Vec<T> {
    #[inline]
    fn is_empty(&self) -> bool {
        Vec::is_empty(self)
    }
}
impl<K, V> IsEmpty for IndexMap<K, V> {
    #[inline]
    fn is_empty(&self) -> bool {
        IndexMap::is_empty(self)
    }
}
impl<K, V> IsEmpty for std::collections::BTreeMap<K, V> {
    #[inline]
    fn is_empty(&self) -> bool {
        std::collections::BTreeMap::is_empty(self)
    }
}
