use crate::i18ndictionary::I18NDictionary;
use crate::NameId;
use serde::{Deserialize, Serialize};

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "typescript", derive(typescript_type_def::TypeDef))]
/// The style map style of a font
pub enum StyleMapStyle {
    /// Bold italic style
    BoldItalic,
    /// Bold style
    Bold,
    /// Regular style
    #[default]
    Regular,
    /// Italic style
    Italic,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(typescript_type_def::TypeDef))]
/// Name table values for a font or individual master
pub struct Names {
    #[serde(default, skip_serializing_if = "I18NDictionary::is_empty")]
    /// Copyright notice (OpenType Name ID 0)
    pub copyright: I18NDictionary,
    #[serde(default, skip_serializing_if = "I18NDictionary::is_empty")]
    /// Font family name (OpenType Name ID 1)
    pub family_name: I18NDictionary,
    #[serde(default, skip_serializing_if = "I18NDictionary::is_empty")]
    /// Font Subfamily name (OpenType Name ID 2)
    pub preferred_subfamily_name: I18NDictionary,
    #[serde(default, skip_serializing_if = "I18NDictionary::is_empty")]
    /// Unique font identifier (OpenType Name ID 3)
    pub unique_id: I18NDictionary,
    #[serde(default, skip_serializing_if = "I18NDictionary::is_empty")]
    /// Full font name (OpenType Name ID 4)
    pub full_name: I18NDictionary,
    #[serde(default, skip_serializing_if = "I18NDictionary::is_empty")]
    /// Version string (OpenType Name ID 5)
    pub version: I18NDictionary,
    #[serde(default, skip_serializing_if = "I18NDictionary::is_empty")]
    /// PostScript name for the font (OpenType Name ID 6)
    pub postscript_name: I18NDictionary,
    #[serde(default, skip_serializing_if = "I18NDictionary::is_empty")]
    /// Trademark (OpenType Name ID 7)
    pub trademark: I18NDictionary,
    #[serde(default, skip_serializing_if = "I18NDictionary::is_empty")]
    /// Manufacturer Name (OpenType Name ID 8)
    pub manufacturer: I18NDictionary,
    #[serde(default, skip_serializing_if = "I18NDictionary::is_empty")]
    /// Designer. (Name of the designer of the typeface.) (OpenType Name ID 9)
    pub designer: I18NDictionary,
    #[serde(default, skip_serializing_if = "I18NDictionary::is_empty")]
    /// Description. (Description of the typeface.) (OpenType Name ID 10)
    pub description: I18NDictionary,
    #[serde(default, skip_serializing_if = "I18NDictionary::is_empty")]
    /// URL of Vendor. URL of font vendor (with protocol, e.g., http://, ftp://). (OpenType Name ID 11)
    pub manufacturer_url: I18NDictionary,
    #[serde(default, skip_serializing_if = "I18NDictionary::is_empty")]
    /// URL of Designer. URL of typeface designer (with protocol, e.g., http://, ftp://). (OpenType Name ID 12)
    pub designer_url: I18NDictionary,
    #[serde(default, skip_serializing_if = "I18NDictionary::is_empty")]
    /// License Description. Description of the license or licenses under which the font is provided. (OpenType Name ID 13)
    pub license: I18NDictionary,
    #[serde(default, skip_serializing_if = "I18NDictionary::is_empty")]
    /// License Info URL. URL where additional licensing information can be found. (OpenType Name ID 14)
    pub license_url: I18NDictionary,
    #[serde(default, skip_serializing_if = "I18NDictionary::is_empty")]
    /// Typographic Family name. (OpenType Name ID 16)
    pub typographic_family: I18NDictionary,
    #[serde(default, skip_serializing_if = "I18NDictionary::is_empty")]
    /// Typographic Subfamily name. (OpenType Name ID 17)
    pub typographic_subfamily: I18NDictionary,
    #[serde(default, skip_serializing_if = "I18NDictionary::is_empty")]
    /// Compatible Full (Macintosh only). (OpenType Name ID 18)
    pub compatible_full_name: I18NDictionary,
    #[serde(default, skip_serializing_if = "I18NDictionary::is_empty")]
    /// Sample text. (OpenType Name ID 19)
    pub sample_text: I18NDictionary,
    #[serde(default, skip_serializing_if = "I18NDictionary::is_empty")]
    /// PostScript CID findfont name. (OpenType Name ID 20)
    pub postscript_cid_name: I18NDictionary, // XXX?
    #[serde(default, skip_serializing_if = "I18NDictionary::is_empty")]
    /// WWS Family Name. (OpenType Name ID 21)
    pub wws_family_name: I18NDictionary,
    #[serde(default, skip_serializing_if = "I18NDictionary::is_empty")]
    /// WWS Subfamily Name. (OpenType Name ID 22)
    pub wws_subfamily_name: I18NDictionary,
    #[serde(default, skip_serializing_if = "I18NDictionary::is_empty")]
    /// Variations PostScript Name Prefix. (OpenType Name ID 25)
    pub variations_postscript_name_prefix: I18NDictionary,
}

impl Names {
    /// Create a new, empty Names struct
    pub fn new() -> Self {
        Self::default()
    }

    /// Get a name by its OpenType Name ID
    pub fn get(&self, stringid: NameId) -> Option<&I18NDictionary> {
        match stringid {
            NameId::COPYRIGHT_NOTICE => Some(&self.copyright),
            NameId::FAMILY_NAME => Some(&self.family_name),
            NameId::SUBFAMILY_NAME => Some(&self.preferred_subfamily_name),
            NameId::UNIQUE_ID => Some(&self.unique_id),
            NameId::FULL_NAME => Some(&self.full_name),
            NameId::VERSION_STRING => Some(&self.version),
            NameId::POSTSCRIPT_NAME => Some(&self.postscript_name),
            NameId::TRADEMARK => Some(&self.trademark),
            NameId::MANUFACTURER => Some(&self.manufacturer),
            NameId::DESIGNER => Some(&self.designer),
            NameId::DESCRIPTION => Some(&self.description),
            NameId::VENDOR_URL => Some(&self.manufacturer_url),
            NameId::DESIGNER_URL => Some(&self.designer_url),
            NameId::LICENSE_DESCRIPTION => Some(&self.license),
            NameId::LICENSE_URL => Some(&self.license_url),
            NameId::TYPOGRAPHIC_FAMILY_NAME => Some(&self.typographic_family),
            NameId::TYPOGRAPHIC_SUBFAMILY_NAME => Some(&self.typographic_subfamily),
            NameId::COMPATIBLE_FULL_NAME => Some(&self.compatible_full_name),
            NameId::SAMPLE_TEXT => Some(&self.sample_text),
            NameId::POSTSCRIPT_CID_NAME => Some(&self.postscript_cid_name),
            NameId::WWS_FAMILY_NAME => Some(&self.wws_family_name),
            NameId::WWS_SUBFAMILY_NAME => Some(&self.wws_subfamily_name),
            NameId::VARIATIONS_POSTSCRIPT_NAME_PREFIX => {
                Some(&self.variations_postscript_name_prefix)
            }
            _ => None,
        }
    }
}
