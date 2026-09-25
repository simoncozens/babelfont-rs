use crate::filters::FontFilter;
use serde_json::json;

/// Where a Glyphs font-level custom parameter lives in `format_specific`.
const CUSTOM_PARAMETERS: &str = "com.schriftgestalt.Glyphs.customParameters.";
const DONT_USE_PRODUCTION_NAMES: &str = "Don't use Production Names";

/// A filter that tells the compiler to keep the glyph names the source states, by
/// setting the Glyphs custom parameter `Don't use Production Names`.
///
/// Without it, a compiler renames glyphs to their production names: `mu` becomes
/// `uni03BC`, `Omega` becomes `uni03A9`, `Delta` becomes `uni0394`, `Tcommaaccent`
/// becomes `uni021A`. FontForge's own TTF export writes each glyph's own name to
/// `post`.
///
/// Opt-in rather than automatic: matching a binary FontForge exported wants the
/// source's names, while a source taken forward as a new master may want the
/// production ones.
///
/// # Against `--no-production-names`
///
/// They make the same request and differ in where it travels. `--no-production-names`
/// is a compilation option: it applies to the TTF babelfont itself produces and
/// leaves no trace in a `.glyphs` output. This filter writes the request into the
/// font as the custom parameter Glyphs itself uses. A `.glyphs`, `.glyphspackage` or
/// `.babelfont` output keeps it, so a later compile of that file honours it; a UFO or
/// designspace output does not carry it. The babelfont command line honours it too
/// when it compiles a TTF; see [`KeepSourceGlyphNames::is_requested`].
#[derive(Default)]
pub struct KeepSourceGlyphNames;

impl KeepSourceGlyphNames {
    /// Create a new KeepSourceGlyphNames filter
    pub fn new() -> Self {
        KeepSourceGlyphNames
    }

    /// Whether `font` carries the Glyphs custom parameter `Don't use Production
    /// Names` enabled and set to 1, written as a number, as a string or as `true`. A
    /// disabled parameter, or one set to 0, asks for nothing.
    pub fn is_requested(font: &crate::Font) -> bool {
        let Some(parameter) = font
            .format_specific
            .get(&format!("{}{}", CUSTOM_PARAMETERS, DONT_USE_PRODUCTION_NAMES))
            .and_then(|v| v.as_object())
        else {
            return false;
        };
        let disabled = parameter
            .get("disabled")
            .and_then(|d| d.as_bool())
            .unwrap_or(false);
        !disabled
            && parameter
                .get("value")
                .is_some_and(|v| {
                    v.as_i64() == Some(1)
                        || v.as_bool() == Some(true)
                        || v.as_str().and_then(|s| s.parse::<i64>().ok()) == Some(1)
                })
    }
}

impl FontFilter for KeepSourceGlyphNames {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        font.format_specific.insert(
            format!("{}{}", CUSTOM_PARAMETERS, DONT_USE_PRODUCTION_NAMES),
            json!({ "value": 1, "disabled": false }),
        );
        Ok(())
    }

    fn from_str(_s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(KeepSourceGlyphNames::new())
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("keepsourceglyphnames")
            .long("keep-source-glyph-names")
            .help(
                "Keep the glyph names the source states instead of letting the compiler \
                 substitute production names (mu -> uni03BC, Omega -> uni03A9), as \
                 FontForge's own TTF export does. Unlike --no-production-names, this \
                 records the request in a .glyphs, .glyphspackage or .babelfont output, \
                 so a later compile of that file honours it too",
            )
            .action(clap::ArgAction::SetTrue)
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::Font;

    #[test]
    fn test_sets_the_glyphs_custom_parameter() {
        let mut font = Font::new();
        assert!(!KeepSourceGlyphNames::is_requested(&font));
        KeepSourceGlyphNames::new().apply(&mut font).unwrap();
        let key = format!("{CUSTOM_PARAMETERS}{DONT_USE_PRODUCTION_NAMES}");
        assert_eq!(
            font.format_specific.get(&key),
            Some(&json!({ "value": 1, "disabled": false }))
        );
        assert!(KeepSourceGlyphNames::is_requested(&font));
    }

    #[test]
    fn test_a_parameter_set_to_0_or_disabled_is_not_a_request() {
        let key = format!("{CUSTOM_PARAMETERS}{DONT_USE_PRODUCTION_NAMES}");
        for (parameter, requested) in [
            (json!({ "value": 1, "disabled": false }), true),
            (json!({ "value": true }), true),
            (json!({ "value": "1", "disabled": false }), true),
            (json!({ "value": 0, "disabled": false }), false),
            (json!({ "value": "0", "disabled": false }), false),
            (json!({ "value": 1, "disabled": true }), false),
        ] {
            let mut font = Font::new();
            font.format_specific.insert(key.clone(), parameter.clone());
            assert_eq!(
                KeepSourceGlyphNames::is_requested(&font),
                requested,
                "{parameter}"
            );
        }
    }
}
