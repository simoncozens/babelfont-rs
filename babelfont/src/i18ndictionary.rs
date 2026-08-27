use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Formatter};
use typeshare::typeshare;

pub(crate) static DFLT: &str = "dflt";

/// A dictionary for internationalized strings.
#[derive(Default, Clone, Serialize, Deserialize, PartialEq)]
#[typeshare]
#[typeshare(serialized_as = "HashMap<String, String>")]
#[cfg_attr(feature = "reactive", derive(reactive_stores::Store))]
pub struct I18NDictionary(pub IndexMap<String, String>);

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

    /// The default string, falling back to English, then to a sole entry.
    ///
    /// For output fields that cannot carry a localization (e.g. Glyphs 3
    /// singular properties), a dictionary populated only under a language
    /// tag -- as an SFD's `LangName` strings are, under `ENG` -- still has
    /// an obvious best value; returning `None` there silently drops it.
    pub fn get_default_or_fallback(&self) -> Option<&String> {
        self.get_default()
            .or_else(|| self.0.get("ENG"))
            .or_else(|| {
                if self.0.len() == 1 {
                    self.0.values().next()
                } else {
                    None
                }
            })
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

impl From<Option<&String>> for I18NDictionary {
    fn from(val: Option<&String>) -> Self {
        let mut f = I18NDictionary::new();
        if let Some(val) = val {
            f.0.insert(DFLT.to_string(), val.to_string());
        }
        f
    }
}

impl From<I18NDictionary> for IndexMap<String, String> {
    fn from(dict: I18NDictionary) -> Self {
        dict.0
    }
}
