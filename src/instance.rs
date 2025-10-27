use crate::{common::FormatSpecific, i18ndictionary::I18NDictionary, names::Names};
use fontdrasil::coords::DesignLocation;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Instance {
    pub id: String,
    pub name: I18NDictionary,
    pub location: DesignLocation,
    pub custom_names: Names,
    pub variable: bool,
    pub linked_style: Option<String>,
    // lib
    pub format_specific: FormatSpecific,
}
