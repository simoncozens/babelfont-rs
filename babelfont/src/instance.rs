use crate::{common::FormatSpecific, i18ndictionary::I18NDictionary, names::Names};
use fontdrasil::coords::DesignLocation;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[typeshare]
/// A font instance
pub struct Instance {
    /// Unique identifier for the instance
    ///
    /// Should be unique within the design space; usually a UUID.
    pub id: String,
    /// Name of the instance
    pub name: I18NDictionary,
    #[serde(
        default,
        serialize_with = "crate::serde_helpers::design_location_to_map",
        deserialize_with = "crate::serde_helpers::design_location_from_map"
    )]
    #[typeshare(python(type = "Dict[str, float]"))]
    #[typeshare(typescript(type = "import('@simoncozens/fonttypes').DesignspaceLocation"))]
    /// Location of the instance in design space coordinates
    pub location: DesignLocation,
    /// Any custom names for the instance if it is exported as a static font
    pub custom_names: Names,
    /// Whether the instance represents an export of a variable font
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub variable: bool,
    /// Name of the linked style for style linking (e.g., "Bold Italic" links to "Bold" and "Italic")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linked_style: Option<String>,
    /// Format-specific data
    #[serde(default, skip_serializing_if = "FormatSpecific::is_empty")]
    #[typeshare(python(type = "Dict[str, Any]"))]
    #[typeshare(typescript(type = "Record<string, any>"))]
    pub format_specific: FormatSpecific,
}
