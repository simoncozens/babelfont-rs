use fontdrasil::coords::DesignLocation;
use indexmap::IndexMap;

use crate::{
    common::{FormatSpecific, OTValue},
    guide::Guide,
    i18ndictionary::I18NDictionary,
    LayerType, MetricType, OTScalar,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(typescript_type_def::TypeDef))]
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
    #[cfg_attr(feature = "typescript", type_def(type_of = "HashMap<String, f32>"))]
    /// Location of the master in design space coordinates
    pub location: DesignLocation,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    /// Global guidelines associated with the master
    pub guides: Vec<Guide>,
    #[cfg_attr(feature = "typescript", type_def(type_of = "HashMap<String, i32>"))]
    /// Master-specific metrics
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
    pub kerning: HashMap<(String, String), i16>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    /// Custom OpenType values for this master
    pub custom_ot_values: Vec<OTValue>,
    #[serde(default, skip_serializing_if = "FormatSpecific::is_empty")]
    /// Format-specific data
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
            guides: vec![],
            metrics: IndexMap::new(),
            kerning: HashMap::new(),
            custom_ot_values: vec![],
            format_specific: FormatSpecific::default(),
        }
    }

    /// Get a custom OpenType value for this master
    pub fn ot_value(&self, table: &str, field: &str) -> Option<OTScalar> {
        for i in &self.custom_ot_values {
            if i.table == table && i.field == field {
                return Some(i.value.clone());
            }
        }
        None
    }

    /// Set a custom OpenType value for this master
    pub fn set_ot_value(&mut self, table: &str, field: &str, value: OTScalar) {
        self.custom_ot_values.push(OTValue {
            table: table.to_string(),
            field: field.to_string(),
            value,
        })
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
