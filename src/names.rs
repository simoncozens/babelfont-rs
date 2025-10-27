use serde::{Deserialize, Serialize};
use write_fonts::read::tables::name::NameId;

use crate::i18ndictionary::I18NDictionary;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StyleMapStyle {
    BoldItalic,
    Bold,
    Regular,
    Italic,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Names {
    pub copyright: I18NDictionary,
    pub family_name: I18NDictionary,
    pub preferred_subfamily_name: I18NDictionary,
    pub unique_id: I18NDictionary,
    pub full_name: I18NDictionary,
    pub version: I18NDictionary,
    pub postscript_name: I18NDictionary,
    pub trademark: I18NDictionary,
    pub manufacturer: I18NDictionary,
    pub designer: I18NDictionary,
    pub description: I18NDictionary,
    pub manufacturer_url: I18NDictionary,
    pub designer_url: I18NDictionary,
    pub license: I18NDictionary,
    pub license_url: I18NDictionary,
    pub reserved: I18NDictionary,
    pub typographic_family: I18NDictionary,
    pub typographic_subfamily: I18NDictionary,
    pub compatible_full_name: I18NDictionary,
    pub sample_text: I18NDictionary,
    pub postscript_cid_name: I18NDictionary, // XXX?
    pub wws_family_name: I18NDictionary,
    pub wws_subfamily_name: I18NDictionary,
}

impl Names {
    pub fn new() -> Self {
        Self::default()
    }

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
            _ => None,
        }
    }
}
