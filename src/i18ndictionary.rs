use std::{
    collections::HashMap,
    fmt::{Debug, Formatter},
};

use serde::{Deserialize, Serialize};

static DFLT: &str = "dflt";

#[derive(Default, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "types", typeshare::typeshare)]
/// A dictionary for internationalized strings.
pub struct I18NDictionary(pub HashMap<String, String>);

impl I18NDictionary {
    /// Create a new, empty I18NDictionary.
    pub fn new() -> Self {
        I18NDictionary::default()
    }

    /// Get the default string, if any.
    pub fn get_default(&self) -> Option<&String> {
        self.0.get(DFLT)
    }

    /// Set the default string.
    pub fn set_default(&mut self, s: String) {
        self.0.insert(DFLT.to_string(), s);
    }

    /// Insert a string for a given language code.
    ///
    /// Language codes should be [OpenType Language System Tags](https://docs.microsoft.com/en-us/typography/opentype/spec/languagetags).
    pub fn insert(&mut self, lang: String, s: String) {
        self.0.insert(lang, s);
    }
    /// Check if the dictionary is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Check if the dictionary has only a single entry.
    pub fn is_single(&self) -> bool {
        self.0.len() == 1
    }
}

impl Debug for I18NDictionary {
    fn fmt(&self, fmt: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        fmt.write_str("<")?;
        let def = self.get_default();
        if let Some(def) = def {
            fmt.write_str(def)?;
        } else {
            fmt.write_str("no default")?;
        }
        fmt.write_str(">")
    }
}

impl From<String> for I18NDictionary {
    fn from(val: String) -> Self {
        let mut f = I18NDictionary::new();
        f.0.insert(DFLT.to_string(), val);
        f
    }
}

impl From<&str> for I18NDictionary {
    fn from(val: &str) -> Self {
        let mut f = I18NDictionary::new();
        f.0.insert(DFLT.to_string(), val.to_string());
        f
    }
}

impl From<&String> for I18NDictionary {
    fn from(val: &String) -> Self {
        let mut f = I18NDictionary::new();
        f.0.insert(DFLT.to_string(), val.to_string());
        f
    }
}

impl From<I18NDictionary> for HashMap<String, String> {
    fn from(dict: I18NDictionary) -> Self {
        dict.0
    }
}
