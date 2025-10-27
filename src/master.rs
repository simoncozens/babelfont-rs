use fontdrasil::coords::DesignLocation;
use indexmap::IndexMap;

use crate::{
    common::{FormatSpecific, OTValue},
    guide::Guide,
    i18ndictionary::I18NDictionary,
    MetricType, OTScalar,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Master {
    pub name: I18NDictionary,
    pub id: String,
    pub location: DesignLocation,
    pub guides: Vec<Guide>,
    pub metrics: IndexMap<MetricType, i32>,
    pub kerning: HashMap<(String, String), i16>,
    pub custom_ot_values: Vec<OTValue>,
    pub format_specific: FormatSpecific,
}

impl Master {
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

    pub fn ot_value(&self, table: &str, field: &str) -> Option<OTScalar> {
        for i in &self.custom_ot_values {
            if i.table == table && i.field == field {
                return Some(i.value.clone());
            }
        }
        None
    }

    pub fn set_ot_value(&mut self, table: &str, field: &str, value: OTScalar) {
        self.custom_ot_values.push(OTValue {
            table: table.to_string(),
            field: field.to_string(),
            value,
        })
    }
}

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
