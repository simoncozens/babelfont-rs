use fontdrasil::coords::DesignLocation;
use indexmap::IndexMap;
use smol_str::SmolStr;
use typeshare::typeshare;

use crate::{
    common::{CustomOTValues, FormatSpecific},
    guide::Guide,
    i18ndictionary::I18NDictionary,
    LayerType, MetricType,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[typeshare]
/// A master/source font in a design space
pub struct Master {
    /// Name of the master
    pub name: I18NDictionary,
    /// Unique identifier for the master (usually a UUID)
    pub id: String,
    #[serde(
        default,
        serialize_with = "crate::serde_helpers::design_location_to_map",
        deserialize_with = "crate::serde_helpers::design_location_from_map"
    )]
    #[typeshare(python(type = "Dict[str, float]"))]
    #[typeshare(typescript(type = "import('@simoncozens/fonttypes').DesignspaceLocation"))]
    /// Location of the master in design space coordinates
    pub location: DesignLocation,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    /// Global guidelines associated with the master
    pub guides: Vec<Guide>,

    /// Master-specific metrics
    #[typeshare(serialized_as = "HashMap<String, i32>")]
    pub metrics: IndexMap<MetricType, i32>,
    /// Kerning for this master.
    ///
    /// (Kerning pairs are (left glyph name, right glyph name) -> value)
    /// Groups are represented as `@<groupname>`; whether they are first or second
    /// groups is determined by position in the tuple.
    #[serde(
        serialize_with = "crate::serde_helpers::kerning_map",
        deserialize_with = "crate::serde_helpers::kerning_unmap"
    )]
    #[typeshare(typescript(type = "Map<[string, string], number>"))]
    #[typeshare(python(type = "Dict[Tuple[str, str], int]"))]
    pub kerning: IndexMap<(SmolStr, SmolStr), i16>,
    /// Custom OpenType values for this master
    #[serde(default, skip_serializing_if = "CustomOTValues::is_empty")]
    pub custom_ot_values: CustomOTValues,
    /// Format-specific data
    #[serde(default, skip_serializing_if = "FormatSpecific::is_empty")]
    #[typeshare(python(type = "Dict[str, Any]"))]
    #[typeshare(typescript(type = "Record<string, any>"))]
    pub format_specific: FormatSpecific,
}

impl Master {
    /// Create a new Master with the given name, id, and location
    pub fn new<T, U>(name: T, id: U, location: DesignLocation) -> Self
    where
        T: Into<I18NDictionary>,
        U: Into<String>,
    {
        Master {
            name: name.into(),
            id: id.into(),
            location,
            guides: Default::default(),
            metrics: Default::default(),
            kerning: Default::default(),
            custom_ot_values: Default::default(),
            format_specific: FormatSpecific::default(),
        }
    }

    /// Check if this master is sparse in the given font (i.e., if any glyphs lack a layer for this master)
    pub fn is_sparse(&self, font: &crate::Font) -> bool {
        for glyph in font.glyphs.iter() {
            let has_layer = glyph
                .layers
                .iter()
                .any(|layer| layer.master == LayerType::DefaultForMaster(self.id.clone()));
            if !has_layer {
                return true;
            }
        }
        false
    }

    // get glyph layer?
    // normalized location?
}

#[cfg(feature = "fontra")]
mod fontra {
    use super::*;
    use crate::convertors::fontra;

    impl From<&Master> for fontra::Source {
        fn from(val: &Master) -> Self {
            fontra::Source {
                name: val.id.clone(),
                // name: val
                //     .name
                //     .get_default()
                //     .map(|x| x.as_str())
                //     .unwrap_or("Unnamed master")
                //     .to_string(),
                is_sparse: "False".to_string(),
                // Location really ought to use axis *name*
                location: val
                    .location
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_f64()))
                    .collect(),
                italic_angle: 0.0,
                guidelines: val
                    .guides
                    .iter()
                    .map(|g| g.into())
                    .collect::<Vec<fontra::Guideline>>(),
                custom_data: HashMap::new(),
            }
        }
    }
}
