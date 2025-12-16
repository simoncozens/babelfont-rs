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
    #[typeshare(typescript(type = "import('fonttypes').DesignspaceLocation"))]
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

#[cfg(feature = "ufo")]
mod ufo {
    use super::Instance;
    use crate::{
        convertors::{designspace::FILENAME_KEY, ufo::KEY_LIB},
        i18ndictionary::I18NDictionary,
    };

    impl From<&Instance> for norad::designspace::Instance {
        fn from(instance: &Instance) -> Self {
            let name_to_option_string = |x: &I18NDictionary| x.get_default().map(|y| y.to_string());
            norad::designspace::Instance {
                familyname: name_to_option_string(&instance.custom_names.family_name),
                stylename: name_to_option_string(&instance.custom_names.preferred_subfamily_name),
                name: name_to_option_string(&instance.name),
                filename: instance
                    .format_specific
                    .get(FILENAME_KEY)
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                postscriptfontname: name_to_option_string(&instance.custom_names.postscript_name),
                stylemapfamilyname: name_to_option_string(
                    &instance.custom_names.typographic_family,
                ),
                stylemapstylename: name_to_option_string(
                    &instance.custom_names.typographic_subfamily,
                ),
                location: instance
                    .location
                    .iter()
                    .map(|(tag, coord)| norad::designspace::Dimension {
                        name: tag.to_string(),
                        uservalue: Some(coord.to_f64() as f32),
                        xvalue: None,
                        yvalue: None,
                    })
                    .collect(),
                lib: serde_json::from_value(
                    instance
                        .format_specific
                        .get(KEY_LIB)
                        .cloned()
                        .unwrap_or_default(),
                )
                .ok()
                .unwrap_or_default(),
            }
        }
    }
}
