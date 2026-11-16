use crate::{convertors::glyphs3::KEY_STYLISTIC_SET_LABEL, filters::FontFilter};
use fea_rs_ast::AsFea;

#[derive(Default)]
/// A filter that converts Glyphs stylistic set labels to OpenType feature code.
pub struct GlyphsStylisticSetLabel;

impl GlyphsStylisticSetLabel {
    /// Create a new GlyphsStylisticSetLabel filter
    pub fn new() -> Self {
        GlyphsStylisticSetLabel
    }
}

impl FontFilter for GlyphsStylisticSetLabel {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        for (_feature_name, feature_code) in font.features.features.iter_mut() {
            if feature_code.code.contains("featureNames") {
                continue;
            }
            if let Some(labels) = feature_code
                .format_specific
                .get(KEY_STYLISTIC_SET_LABEL)
                .and_then(|x| x.as_array())
            {
                // Convert Glyphs stylistic set labels to OpenType feature code
                // Oops, we currently don't have the ability to create a new NestedBlock, do it manually.
                let mut new_statements = Vec::new();
                for label in labels.iter().flat_map(|x| x.as_object()) {
                    let mut language = label
                        .get("language")
                        .and_then(|v| v.as_str())
                        .unwrap_or("ENG ")
                        .to_string();
                    if language == "dflt" {
                        language = "ENG ".to_string();
                    }
                    let value = label
                        .get("value")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    let (mac_lang_id, win_lang_id) = tag_to_ids(&language);

                    if let Some(lang_id) = win_lang_id {
                        let platform_id = 3;
                        let plat_enc_id = 1;
                        let statement = fea_rs_ast::NameRecord::new(
                            platform_id,
                            plat_enc_id,
                            lang_id,
                            value,
                            fea_rs_ast::NameRecordKind::FeatureName,
                            0..0,
                        );
                        new_statements.push(statement);
                    } else if let Some(lang_id) = mac_lang_id {
                        let platform_id = 1;
                        let plat_enc_id = 0;
                        let statement = fea_rs_ast::NameRecord::new(
                            platform_id,
                            plat_enc_id,
                            lang_id,
                            value.clone(),
                            fea_rs_ast::NameRecordKind::FeatureName,
                            0..0,
                        );
                        new_statements.push(statement);
                    }
                }
                feature_code.code = format!(
                    "featureNames {{\n{}\n}};\n{}",
                    new_statements
                        .iter()
                        .map(|s| s.as_fea("  "))
                        .collect::<Vec<_>>()
                        .join("\n"),
                    feature_code.code
                );
            }
        }
        Ok(())
    }

    fn from_str(_s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(GlyphsStylisticSetLabel::new())
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("glyphsstylisticsetlabel")
            .long("convert-glyphs-stylistic-set-labels")
            .help("Convert Glyphs stylistic set labels to OpenType feature code")
            .action(clap::ArgAction::SetTrue)
    }
}

/// Returns `(mac_language_id, windows_lcid)`.
///
/// `LANGUAGE_TAGS` stores `(tag, windows_lcid, mac_language_id)` — ENG is
/// Windows 1033 (0x0409) and Mac 0. Mixing those up emits Windows name records
/// with language 0, which never match the usual 3/1/0x0409 lookup.
fn tag_to_ids(tag: &str) -> (Option<u16>, Option<u16>) {
    let tag = format!("{:<4}", tag); // Pad to 4 characters
    for (t, win_id, mac_id) in LANGUAGE_TAGS {
        if *t == tag {
            return (mac_id.map(|id| id as u16), win_id.map(|id| id as u16));
        }
    }
    (None, None)
}

/// `(OpenType language tag, Windows LCID, Macintosh language ID)`
const LANGUAGE_TAGS: &[(&str, Option<u32>, Option<u32>)] = &[
    ("AFK ", Some(1078), Some(141)),
    ("ALS ", Some(1156), None),
    ("AMH ", Some(1118), Some(85)),
    ("ARA ", Some(1025), Some(12)),
    ("ASM ", Some(1101), Some(68)),
    ("AYM ", None, Some(134)),
    ("BEL ", Some(1059), None),
    ("BGR ", Some(1026), Some(44)),
    ("BOS ", Some(5146), None),
    ("BRE ", Some(1150), Some(142)),
    ("BRM ", None, Some(77)),
    ("BSH ", Some(1133), None),
    ("CAT ", Some(1027), Some(130)),
    ("COS ", Some(1155), None),
    ("CSY ", Some(1029), Some(38)),
    ("DAN ", Some(1030), Some(7)),
    ("DEU ", Some(1031), Some(2)),
    ("DRI ", Some(1164), None),
    ("DZN ", None, Some(137)),
    ("ELL ", Some(1032), Some(14)),
    ("ENG ", Some(1033), Some(0)),
    ("ESP ", Some(1034), Some(6)),
    ("ETI ", Some(1061), Some(27)),
    ("EUQ ", Some(1069), Some(129)),
    ("FAR ", Some(1065), None),
    ("FIN ", Some(1035), Some(13)),
    ("FOS ", Some(1080), Some(30)),
    ("FRA ", Some(1036), Some(1)),
    ("FRI ", Some(1122), None),
    ("GAE ", Some(1169), None),
    ("GAL ", Some(1110), Some(140)),
    ("GRN ", Some(1135), Some(149)),
    ("GUA ", None, Some(133)),
    ("GUJ ", Some(1095), Some(69)),
    ("HIN ", Some(1081), Some(21)),
    ("HRV ", Some(1050), Some(18)),
    ("HUN ", Some(1038), Some(26)),
    ("HYE ", Some(1067), Some(51)),
    ("IBO ", Some(1136), None),
    ("IND ", Some(1057), Some(81)),
    ("INU ", None, Some(143)),
    ("IRI ", Some(2108), None),
    ("ISL ", Some(1039), Some(15)),
    ("ITA ", Some(1040), Some(3)),
    ("IWR ", Some(1037), Some(10)),
    ("JAN ", Some(1041), Some(11)),
    ("JII ", None, Some(41)),
    ("KAN ", Some(1099), Some(73)),
    ("KAT ", Some(1079), Some(52)),
    ("KAZ ", Some(1087), Some(48)),
    ("KHM ", Some(1107), Some(78)),
    ("KOK ", Some(1111), None),
    ("KOR ", Some(1042), Some(23)),
    ("KSH ", None, Some(61)),
    ("KUR ", None, Some(60)),
    ("LAO ", Some(1108), Some(79)),
    ("LAT ", None, Some(131)),
    ("LSB ", Some(2094), None),
    ("LTH ", Some(1063), Some(24)),
    ("LTZ ", Some(1134), None),
    ("LVI ", Some(1062), Some(28)),
    ("MAL ", Some(1100), Some(72)),
    ("MAP ", Some(1146), None),
    ("MAR ", Some(1102), Some(66)),
    ("MKD ", None, Some(43)),
    ("MLG ", None, Some(93)),
    ("MLY ", Some(1086), None),
    ("MOH ", Some(1148), None),
    ("MOL ", None, Some(53)),
    ("MRI ", Some(1153), None),
    ("MTS ", Some(1082), Some(16)),
    ("NEP ", Some(1121), Some(64)),
    ("NLD ", Some(1043), Some(4)),
    ("NOR ", None, Some(9)),
    ("NTO ", None, Some(94)),
    ("OCI ", Some(1154), None),
    ("ORO ", None, Some(87)),
    ("PAN ", Some(1094), Some(70)),
    ("PAS ", Some(1123), Some(59)),
    ("PIL ", Some(1124), None),
    ("PLK ", Some(1045), Some(25)),
    ("PTG ", Some(1046), Some(8)),
    ("QUZ ", Some(1131), Some(132)),
    ("RMS ", Some(1047), None),
    ("ROM ", Some(1048), Some(37)),
    ("RUA ", Some(1159), None),
    ("RUN ", None, Some(91)),
    ("RUS ", Some(1049), Some(32)),
    ("SAN ", Some(1103), Some(65)),
    ("SKY ", Some(1051), Some(39)),
    ("SLV ", Some(1060), Some(40)),
    ("SML ", None, Some(88)),
    ("SND ", None, Some(62)),
    ("SQI ", Some(1052), Some(36)),
    ("SRB ", None, Some(42)),
    ("SVE ", Some(1053), Some(5)),
    ("SWK ", None, Some(89)),
    ("SYR ", Some(1114), None),
    ("TAJ ", None, Some(55)),
    ("TAM ", Some(1097), Some(74)),
    ("TAT ", Some(1092), Some(135)),
    ("TEL ", Some(1098), Some(75)),
    ("TGL ", None, Some(82)),
    ("TGN ", None, Some(147)),
    ("TGY ", None, Some(86)),
    ("THA ", Some(1054), Some(22)),
    ("TIB ", Some(1105), Some(63)),
    ("TKM ", Some(1090), Some(56)),
    ("TRK ", Some(1055), Some(17)),
    ("UKR ", Some(1058), Some(45)),
    ("URD ", Some(1056), Some(20)),
    ("USB ", Some(1070), None),
    ("UYG ", Some(1152), None),
    ("UZB ", Some(1091), Some(47)),
    ("VIT ", Some(1066), Some(80)),
    ("WEL ", Some(1106), Some(128)),
    ("WLF ", Some(1160), None),
    ("YBA ", Some(1130), None),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convertors::glyphs3::KEY_STYLISTIC_SET_LABEL;
    use crate::{features::PossiblyAutomaticCode, Font};

    #[test]
    fn english_maps_to_windows_lcid_1033() {
        assert_eq!(tag_to_ids("ENG"), (Some(0), Some(1033)));
        assert_eq!(tag_to_ids("ENG "), (Some(0), Some(1033)));
    }

    #[test]
    fn label_emits_windows_english_not_language_zero() {
        let mut font = Font::new();
        let mut code = PossiblyAutomaticCode::new("sub a by a.ss01;");
        code.format_specific.insert(
            KEY_STYLISTIC_SET_LABEL.into(),
            serde_json::json!([{ "language": "ENG", "value": "Geometric a g" }]),
        );
        font.features.features.push(("ss01".into(), code));
        assert!(
            GlyphsStylisticSetLabel.apply(&mut font).is_ok(),
            "label conversion"
        );
        let fea = &font.features.features[0].1.code;
        assert!(fea.contains("featureNames"), "{fea}");
        assert!(fea.contains("Geometric a g"), "{fea}");
        assert!(
            !fea.contains("name 3 1 0 "),
            "Windows English must use LCID 1033, not language 0:\n{fea}"
        );
    }
}
