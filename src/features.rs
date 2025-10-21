use indexmap::IndexMap;
use smol_str::SmolStr;

#[derive(Debug, Clone, Default)]
pub struct Features {
    /// Opentype classes classes
    ///
    /// Each group is a list of glyph names or class names. The key should not start with @.
    pub classes: IndexMap<SmolStr, Vec<String>>,
    /// Opentype prefixes
    ///
    /// A dictionary of OpenType lookups and other feature code to be placed before features are defined.
    /// The keys are user-defined names, the values are AFDKO feature code.
    pub prefixes: IndexMap<SmolStr, String>,
    /// OpenType features
    ///
    /// A list of OpenType feature code, expressed as a tuple (feature tag, code).
    pub features: Vec<(SmolStr, String)>,
}

impl Features {
    pub fn to_fea(&self) -> String {
        let mut fea = String::new();
        for (name, glyphs) in &self.classes {
            fea.push_str(&format!("@{} = [{}];\n", name, glyphs.join(" ")));
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

    pub(crate) fn from_fea(fea: &str) -> Features {
        // A very naive parser that just puts everything into the anonymous prefix.
        let mut features = Features::default();
        features
            .prefixes
            .insert("anonymous".into(), fea.to_string());
        features
    }
}
