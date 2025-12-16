use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;
use typeshare::typeshare;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[typeshare]
/// A representation of OpenType features, classes, and prefixes.
pub struct Features {
    /// Opentype classes
    ///
    /// The key should not start with @.
    #[typeshare(serialized_as = "HashMap<String, String>")]
    pub classes: IndexMap<SmolStr, String>,
    /// Opentype prefixes
    ///
    /// A dictionary of OpenType lookups and other feature code to be placed before features are defined.
    /// The keys are user-defined names, the values are AFDKO feature code.
    #[typeshare(serialized_as = "HashMap<String, String>")]
    pub prefixes: IndexMap<SmolStr, String>,
    /// OpenType features
    ///
    /// A list of OpenType feature code, expressed as a tuple (feature tag, code).
    #[typeshare(python(type = "List[Tuple[str, str]]"))]
    #[typeshare(typescript(type = "Array<[string, string]>"))]
    pub features: Vec<(SmolStr, String)>,
    /// Include paths
    ///
    /// Paths to search for included feature files.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[typeshare(python(type = "List[str]"))]
    #[typeshare(typescript(type = "string[]"))]
    pub include_paths: Vec<std::path::PathBuf>,
}

impl Features {
    /// Serialize to a single string of AFDKO feature code.
    pub fn to_fea(&self) -> String {
        let mut fea = String::new();
        for (name, glyphs) in &self.classes {
            fea.push_str(&format!("@{} = [{}];\n", name, glyphs));
        }
        for (prefix, code) in &self.prefixes {
            if prefix != "anonymous" {
                fea.push_str(&format!("# Prefix: {}\n", prefix));
            }
            fea.push_str(code);
            fea.push('\n');
        }
        for (name, code) in &self.features {
            fea.push_str(&format!("feature {} {{\n{}\n}} {};\n", name, code, name));
        }
        fea
    }

    /// A very naive parser that just puts everything into the anonymous prefix.
    pub fn from_fea(fea: &str) -> Features {
        let mut features = Features::default();
        features
            .prefixes
            .insert("anonymous".into(), fea.to_string());
        features
    }
}
