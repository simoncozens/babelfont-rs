use crate::filters::FontFilter;

/// fsSelection bits 7 (`USE_TYPO_METRICS`) and 8 (`WWS`), defined from OS/2 version 4.
const VERSION_4_BITS: u16 = (1 << 7) | (1 << 8);

#[derive(Default)]
/// A filter that clears fsSelection bits 7 and 8 when a FontForge source states an
/// `OS2Version` from 1 to 3.
///
/// FontForge up to tag 20141230 wrote the stated version as is, and these bits exist
/// only from version 4, so a binary it exported does not have them.
pub struct FontForgeLegacyOs2Version;

impl FontForgeLegacyOs2Version {
    /// Create a new FontForgeLegacyOs2Version filter
    pub fn new() -> Self {
        FontForgeLegacyOs2Version
    }
}

impl FontFilter for FontForgeLegacyOs2Version {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        let version = font
            .format_specific
            .get("OS2Version")
            .and_then(|v| v.as_str())
            .and_then(|v| v.trim().parse::<u16>().ok())
            .unwrap_or(0);
        if !(1..4).contains(&version) {
            return Ok(());
        }
        if let Some(selection) = font.custom_ot_values.os2_fs_selection.as_mut() {
            *selection &= !VERSION_4_BITS;
        }
        Ok(())
    }

    fn from_str(_s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(FontForgeLegacyOs2Version::new())
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("fontforgelegacyos2version")
            .long("fontforge-legacy-os2-version")
            .help(
                "Clear fsSelection bits 7 and 8 when a FontForge source states an OS2Version \
                 from 1 to 3, as FontForge up to tag 20141230 did",
            )
            .action(clap::ArgAction::SetTrue)
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::Font;

    fn font_with(version: Option<&str>, selection: u16) -> Font {
        let mut font = Font::new();
        if let Some(version) = version {
            font.format_specific.insert(
                "OS2Version".to_string(),
                serde_json::Value::String(version.to_string()),
            );
        }
        font.custom_ot_values.os2_fs_selection = Some(selection);
        font
    }

    #[test]
    fn test_version_below_4_clears_bits_7_and_8() {
        let mut font = font_with(Some("2"), 0x40 | 0x80 | 0x100);
        FontForgeLegacyOs2Version::new().apply(&mut font).unwrap();
        assert_eq!(font.custom_ot_values.os2_fs_selection, Some(0x40));
    }

    #[test]
    fn test_version_0_4_or_absent_is_left_alone() {
        for version in [Some("0"), Some("4"), None] {
            let mut font = font_with(version, 0x180);
            FontForgeLegacyOs2Version::new().apply(&mut font).unwrap();
            assert_eq!(font.custom_ot_values.os2_fs_selection, Some(0x180));
        }
    }
}
