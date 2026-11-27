use crate::{convertors::glyphs3::KEY_CUSTOM_PARAMETERS, filters::FontFilter};

/// A filter that asks a Glyphs compiler not to propagate anchors from components.
///
/// FontForge exports a glyph's own anchor points only: a glyph made of references
/// and carrying no anchors of its own is not a base in its mark lookups. A Glyphs
/// compiler copies anchors up from components unless the font-level custom
/// parameter "Propagate Anchors" is false, which this filter sets.
#[derive(Default)]
pub struct FontForgeNoAnchorPropagation;

impl FontForgeNoAnchorPropagation {
    /// Create a new FontForgeNoAnchorPropagation filter
    pub fn new() -> Self {
        FontForgeNoAnchorPropagation
    }
}

impl FontFilter for FontForgeNoAnchorPropagation {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        font.format_specific.insert(
            format!("{KEY_CUSTOM_PARAMETERS}Propagate Anchors"),
            serde_json::json!({ "value": 0, "disabled": false }),
        );
        Ok(())
    }

    fn from_str(_s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(FontForgeNoAnchorPropagation::new())
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("fontforgenoanchorpropagation")
            .long("fontforge-no-anchor-propagation")
            .help(
                "Set the Glyphs custom parameter Propagate Anchors to false: FontForge \
                 exports a glyph's own anchors only, never its components'",
            )
            .action(clap::ArgAction::SetTrue)
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(all(test, feature = "glyphs"))]
mod tests {
    use super::*;
    use crate::Font;

    #[test]
    fn test_glyphs_file_turns_anchor_propagation_off() {
        let mut font = Font::new();
        FontForgeNoAnchorPropagation::new().apply(&mut font).unwrap();
        let glyphs = crate::convertors::glyphs3::as_glyphs3(&font).unwrap();
        let parameter = glyphs
            .custom_parameters
            .iter()
            .find(|cp| cp.name == "Propagate Anchors")
            .expect("the font states Propagate Anchors");
        assert!(!parameter.disabled);
        assert_eq!(
            serde_json::to_value(&parameter.value).unwrap(),
            serde_json::json!(0)
        );
    }
}
