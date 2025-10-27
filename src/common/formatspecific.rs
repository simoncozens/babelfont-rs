use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct FormatSpecific(IndexMap<String, Value>);

impl FormatSpecific {
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.get(key)
    }

    pub fn insert(&mut self, key: String, value: Value) {
        self.0.insert(key, value);
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }

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

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.0.iter()
    }
}
