use std::collections::HashMap;

pub use crate::Tag;
use crate::{common::FormatSpecific, i18ndictionary::I18NDictionary, BabelfontError};
use fontdrasil::coords::{
    CoordConverter, DesignCoord, DesignSpace, Location, NormalizedCoord, UserCoord,
};
use serde::{Deserialize, Serialize};
use typeshare::typeshare;

/// An axis in a variable font
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "reactive", derive(reactive_stores::Store))]
#[typeshare]
pub struct Axis {
    /// Name of the axis
    pub name: I18NDictionary,
    #[typeshare(serialized_as = "String")]
    /// 4-character tag of the axis
    pub tag: Tag,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serde_helpers::usercoord_option_ser",
        deserialize_with = "crate::serde_helpers::usercoord_option_de"
    )]
    #[typeshare(python(type = "Optional[float]"))]
    #[typeshare(typescript(type = "import('@simoncozens/fonttypes').UserspaceCoordinate"))]
    /// Minimum value of the axis in user space coordinates
    pub min: Option<UserCoord>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serde_helpers::usercoord_option_ser",
        deserialize_with = "crate::serde_helpers::usercoord_option_de"
    )]
    #[typeshare(python(type = "Optional[float]"))]
    #[typeshare(typescript(type = "import('@simoncozens/fonttypes').UserspaceCoordinate"))]
    /// Maximum value of the axis in user space coordinates
    pub max: Option<UserCoord>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serde_helpers::usercoord_option_ser",
        deserialize_with = "crate::serde_helpers::usercoord_option_de"
    )]
    #[typeshare(python(type = "Optional[float]"))]
    #[typeshare(typescript(type = "import('@simoncozens/fonttypes').UserspaceCoordinate"))]
    /// Default value of the axis in user space coordinates
    pub default: Option<UserCoord>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serde_helpers::axismap_ser",
        deserialize_with = "crate::serde_helpers::axismap_de"
    )]
    #[typeshare(python(type = "Optional[List[Tuple[float, float]]]"))]
    #[typeshare(typescript(
        type = "Array<[import('@simoncozens/fonttypes').UserspaceCoordinate, import('@simoncozens/fonttypes').DesignspaceCoordinate]> | null"
    ))]
    /// Mapping of user space coordinates to design space coordinates
    pub map: Option<Vec<(UserCoord, DesignCoord)>>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    /// Whether the axis is hidden in the font's user interface
    pub hidden: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[typeshare(python(type = "List[float]"))]
    #[typeshare(typescript(type = "import('@simoncozens/fonttypes').UserspaceCoordinate[]"))]
    /// Predefined values for the axis in user space coordinates
    pub values: Vec<UserCoord>,
    /// Format-specific data
    #[serde(default, skip_serializing_if = "FormatSpecific::is_empty")]
    #[typeshare(python(type = "Dict[str, Any]"))]
    #[typeshare(typescript(type = "Record<string, any>"))]
    pub format_specific: FormatSpecific,
}

impl Axis {
    /// Create a new axis with the given name and tag
    pub fn new<T>(name: T, tag: Tag) -> Self
    where
        T: Into<I18NDictionary>,
    {
        Axis {
            name: name.into(),
            tag,
            ..Default::default()
        }
    }

    pub(crate) fn _converter(&self) -> Result<CoordConverter, BabelfontError> {
        if let Some(map) = self.map.as_ref() {
            // Find default
            let default_idx = map
                .iter()
                .position(|(coord, _)| Some(coord) == self.default.as_ref())
                .ok_or_else(|| BabelfontError::IllDefinedAxis {
                    axis_name: self.name(),
                    reason: "Default value not in map".to_string(),
                })?;
            Ok(CoordConverter::new(map.to_vec(), default_idx)?)
        } else {
            let (min, default, max) =
                self.bounds()
                    .ok_or_else(|| BabelfontError::IllDefinedAxis {
                        axis_name: self.name(),
                        reason: "Missing min, default, or max".to_string(),
                    })?;
            Ok(CoordConverter::unmapped(min, default, max))
        }
    }

    /// Get the bounds (min, default, max) of this axis, if all are defined
    pub fn bounds(&self) -> Option<(UserCoord, UserCoord, UserCoord)> {
        if self.min.is_none() || self.default.is_none() || self.max.is_none() {
            // If we have a list of values, use that as min/max
            #[allow(clippy::unwrap_used)] // If any value is NaN, user deserves what they get
            if !self.values.is_empty() {
                let min = self
                    .values
                    .iter()
                    .cloned()
                    .min_by(|a, b| a.partial_cmp(b).unwrap());
                let max = self
                    .values
                    .iter()
                    .cloned()
                    .max_by(|a, b| a.partial_cmp(b).unwrap());
                if let (Some(min), Some(max)) = (min, max) {
                    let default = self.default.unwrap_or(min);
                    return Some((min, default, max));
                }
            }
            return None;
        }
        #[allow(clippy::unwrap_used)] // We just checked that!
        Some((self.min.unwrap(), self.default.unwrap(), self.max.unwrap()))
    }

    /// Converts a position on this axis from designspace coordinates to userspace coordinates
    pub fn designspace_to_userspace(&self, l: DesignCoord) -> Result<UserCoord, BabelfontError> {
        self._converter().map(|c| l.convert(&c))
    }

    /// Converts a position on this axis in userspace coordinates to designspace coordinates
    pub fn userspace_to_designspace(&self, l: UserCoord) -> Result<DesignCoord, BabelfontError> {
        self._converter().map(|c| l.convert(&c))
    }

    /// Normalize a userspace value to -1.0 to 1.0 range
    pub fn normalize_userspace_value(
        &self,
        l: UserCoord,
    ) -> Result<NormalizedCoord, BabelfontError> {
        self._converter().map(|c| l.convert(&c))
    }

    /// Normalize a designspace value to -1.0 to 1.0 range
    pub fn normalize_designspace_value(
        &self,
        l: DesignCoord,
    ) -> Result<NormalizedCoord, BabelfontError> {
        self._converter().map(|c| l.convert(&c))
    }

    // xxx denormalize functions?

    /// Get the name of the axis in the default language, or "Unnamed axis" if not set
    pub fn name(&self) -> String {
        self.name
            .get_default()
            .unwrap_or(&"Unnamed axis".to_string())
            .to_string()
    }
}

impl TryInto<fontdrasil::types::Axis> for Axis {
    type Error = BabelfontError;

    fn try_into(self) -> Result<fontdrasil::types::Axis, Self::Error> {
        let (min, default, max) = self
            .bounds()
            .ok_or_else(|| BabelfontError::IllDefinedAxis {
                axis_name: self
                    .name
                    .get_default()
                    .unwrap_or(&"Unnamed axis".to_string())
                    .to_string(),
                reason: "Missing min, default, or max".to_string(),
            })?;
        let converter = self._converter()?;
        Ok(fontdrasil::types::Axis {
            name: self.name(),
            tag: self.tag,
            min,
            max,
            default,
            hidden: self.hidden,
            localized_names: HashMap::new(),
            converter,
        })
    }
}

impl TryInto<fontdrasil::types::Axis> for &Axis {
    type Error = BabelfontError;
    fn try_into(self) -> Result<fontdrasil::types::Axis, Self::Error> {
        let (min, default, max) = self
            .bounds()
            .ok_or_else(|| BabelfontError::IllDefinedAxis {
                axis_name: self
                    .name
                    .get_default()
                    .unwrap_or(&"Unnamed axis".to_string())
                    .to_string(),
                reason: "Missing min, default, or max".to_string(),
            })?;
        let converter = self._converter()?;
        Ok(fontdrasil::types::Axis {
            name: self.name(),
            tag: self.tag,
            min,
            max,
            default,
            hidden: self.hidden,
            converter,
            localized_names: HashMap::new(),
        })
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[typeshare]
pub struct CrossAxisMapping {
    /// Description
    pub description: Option<String>,
    /// Source designspace location
    #[typeshare(python(type = "Dict[str, float]]"))]
    #[typeshare(typescript(type = "import('@simoncozens/fonttypes').DesignspaceLocation"))]
    #[serde(
        default,
        serialize_with = "crate::serde_helpers::design_location_to_map",
        deserialize_with = "crate::serde_helpers::design_location_from_map"
    )]
    pub input: Location<DesignSpace>,
    /// Target designspace location
    #[typeshare(python(type = "Dict[str, float]]"))]
    #[typeshare(typescript(type = "import('@simoncozens/fonttypes').DesignspaceLocation"))]
    #[serde(
        default,
        serialize_with = "crate::serde_helpers::design_location_to_map",
        deserialize_with = "crate::serde_helpers::design_location_from_map"
    )]
    pub output: Location<DesignSpace>,
    /// Whether the mapping is active
    #[serde(
        skip_serializing_if = "crate::serde_helpers::is_true",
        default = "crate::serde_helpers::default_true"
    )]
    pub active: bool,
}

#[cfg(feature = "fontra")]
pub(crate) mod fontra {
    use std::collections::HashMap;

    use fontdrasil::coords::Location;

    use super::{Axis, DesignCoord, UserCoord};
    use crate::{axis::CrossAxisMapping, convertors::fontra, BabelfontError, Tag};

    impl TryFrom<&Axis> for fontra::FontAxis {
        type Error = BabelfontError;

        fn try_from(value: &Axis) -> Result<Self, Self::Error> {
            let converter = value._converter()?;
            Ok(fontra::FontAxis {
                name: value.tag.to_string(), // XXX: This should be the name, but for expediency
                label: "".to_string(),
                tag: value.tag.to_string(),
                min_value: value.min.map(|x| x.to_f64()).unwrap_or(0.0),
                max_value: value.max.map(|x| x.to_f64()).unwrap_or(0.0),
                default_value: value.default.map(|x| x.to_f64()).unwrap_or(0.0),
                hidden: value.hidden,
                mapping: value
                    .map
                    .as_ref()
                    .map(|map| {
                        map.iter()
                            .map(|(u, d)| [u.to_f64(), d.to_normalized(&converter).to_f64()])
                            .collect::<Vec<[f64; 2]>>()
                    })
                    .unwrap_or_default(),
                value_labels: vec![],
                custom_data: HashMap::new(),
            })
        }
    }

    impl From<&fontra::FontAxis> for Axis {
        fn from(value: &fontra::FontAxis) -> Self {
            let map = if value.mapping.is_empty() {
                None
            } else {
                Some(
                    value
                        .mapping
                        .iter()
                        .map(|[u, d]| (UserCoord::new(*u), DesignCoord::new(*d)))
                        .collect(),
                )
            };
            Axis {
                name: value.name.clone().into(),
                tag: Tag::new(value.tag.as_bytes()[0..4].try_into().unwrap()),
                min: Some(UserCoord::new(value.min_value)),
                max: Some(UserCoord::new(value.max_value)),
                default: Some(UserCoord::new(value.default_value)),
                hidden: value.hidden,
                map,
                values: vec![],
                format_specific: Default::default(),
            }
        }
    }

    // We store cross-axis mappings using axis tags, Fontra uses names. So we need a mapping between the two when converting
    pub(crate) fn cross_axis_mapping_from_fontra(
        value: &fontra::CrossAxisMapping,
        axes: &[Axis],
    ) -> Result<CrossAxisMapping, BabelfontError> {
        let name_tag_map = axes
            .iter()
            .map(|ax| {
                (
                    ax.name
                        .get_default()
                        .map(|x| x.clone())
                        .unwrap_or(ax.tag.to_string()),
                    ax.tag.clone(),
                )
            })
            .collect::<HashMap<_, _>>();
        let mut input = Location::default();
        for (name, value) in &value.input_location {
            if let Some(tag) = name_tag_map.get(name) {
                input.insert(tag.clone(), DesignCoord::new(*value));
            } else {
                return Err(BabelfontError::AxisConversion(format!(
                    "Unknown axis: {}",
                    name
                )));
            }
        }
        let mut output = Location::default();
        for (name, value) in &value.output_location {
            if let Some(tag) = name_tag_map.get(name) {
                output.insert(tag.clone(), DesignCoord::new(*value));
            } else {
                return Err(BabelfontError::AxisConversion(format!(
                    "Unknown axis: {}",
                    name
                )));
            }
        }
        Ok(CrossAxisMapping {
            description: value.description.clone(),
            input,
            output,
            active: !value.inactive,
        })
    }

    fn cross_axis_mapping_to_fontra(
        mapping: &CrossAxisMapping,
        axes: &[Axis],
    ) -> Result<fontra::CrossAxisMapping, BabelfontError> {
        let tag_name_map = axes
            .iter()
            .map(|ax| {
                (
                    ax.tag.clone(),
                    ax.name
                        .get_default()
                        .map(|n| n.clone())
                        .unwrap_or(ax.tag.to_string()),
                )
            })
            .collect::<HashMap<_, _>>();

        let mut input_location = HashMap::new();
        for (tag, value) in mapping.input.iter() {
            if let Some(name) = tag_name_map.get(tag) {
                input_location.insert(name.clone(), value.to_f64());
            } else {
                return Err(BabelfontError::AxisConversion(format!(
                    "Unknown axis tag: {}",
                    tag
                )));
            }
        }

        let mut output_location = HashMap::new();
        for (tag, value) in mapping.output.iter() {
            if let Some(name) = tag_name_map.get(tag) {
                output_location.insert(name.clone(), value.to_f64());
            } else {
                return Err(BabelfontError::AxisConversion(format!(
                    "Unknown axis tag: {}",
                    tag
                )));
            }
        }

        Ok(fontra::CrossAxisMapping {
            description: mapping.description.clone(),
            group_description: None,
            input_location,
            output_location,
            inactive: !mapping.active,
        })
    }
}

#[cfg(feature = "ufo")]
mod ufo {
    use fontdrasil::coords::{DesignCoord, UserCoord};

    use crate::{common::tag_from_string, BabelfontError};

    use super::Axis;
    impl TryFrom<&norad::designspace::Axis> for Axis {
        type Error = BabelfontError;
        fn try_from(dsax: &norad::designspace::Axis) -> Result<Self, BabelfontError> {
            let mut ax = Axis::new(dsax.name.clone(), tag_from_string(&dsax.tag)?);
            ax.min = dsax.minimum.map(|x| x as f64).map(UserCoord::new);
            ax.max = dsax.maximum.map(|x| x as f64).map(UserCoord::new);
            ax.default = Some(UserCoord::new(dsax.default as f64));
            if let Some(values) = &dsax.values {
                ax.values = values.iter().map(|x| UserCoord::new(*x as f64)).collect();
            }
            if let Some(map) = &dsax.map {
                ax.map = Some(
                    map.iter()
                        .map(|x| {
                            (
                                UserCoord::new(x.input as f64),
                                DesignCoord::new(x.output as f64),
                            )
                        })
                        .collect(),
                );
            }
            ax.hidden = dsax.hidden;
            Ok(ax)
        }
    }

    impl From<&Axis> for norad::designspace::Axis {
        fn from(ax: &Axis) -> Self {
            norad::designspace::Axis {
                name: ax
                    .name
                    .get_default()
                    .unwrap_or(&"Unnamed axis".to_string())
                    .clone(),
                tag: ax.tag.to_string(),
                minimum: ax.min.map(|x| x.to_f64() as f32),
                maximum: ax.max.map(|x| x.to_f64() as f32),
                default: ax.default.map(|x| x.to_f64() as f32).unwrap_or(0.0),
                map: ax.map.as_ref().map(|mapping| {
                    mapping
                        .iter()
                        .map(|(input, output)| norad::designspace::AxisMapping {
                            input: input.to_f64() as f32,
                            output: output.to_f64() as f32,
                        })
                        .collect()
                }),
                hidden: ax.hidden,
                values: (!ax.values.is_empty())
                    .then(|| ax.values.iter().map(|v| v.to_f64() as f32).collect()),
                label_names: vec![],
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    macro_rules! uc {
        ($val:expr) => {
            UserCoord::new($val)
        };
    }
    macro_rules! dc {
        ($val:expr) => {
            DesignCoord::new($val)
        };
    }

    #[test]
    fn test_linear_map() {
        let mut weight = Axis::new("Weight".to_string(), Tag::from_be_bytes(*b"wght"));
        weight.min = Some(uc!(100.0));
        weight.max = Some(uc!(900.0));
        weight.default = Some(uc!(100.0));
        weight.map = Some(vec![(uc!(100.0), dc!(10.0)), (uc!(900.0), dc!(90.0))]);

        assert_eq!(weight.userspace_to_designspace(uc!(400.0)).unwrap(), 40.0);
        assert_eq!(
            weight.designspace_to_userspace(dc!(40.0)).unwrap(),
            uc!(400.0)
        );
    }

    #[test]
    fn test_nonlinear_map() {
        let mut weight = Axis::new("Weight".to_string(), Tag::from_be_bytes(*b"wght"));
        weight.min = Some(uc!(200.0));
        weight.max = Some(uc!(1000.0));
        weight.default = Some(uc!(200.0));
        weight.map = Some(vec![
            (uc!(200.0), dc!(42.0)),
            (uc!(300.0), dc!(61.0)),
            (uc!(400.0), dc!(81.0)),
            (uc!(600.0), dc!(101.0)),
            (uc!(700.0), dc!(125.0)),
            (uc!(800.0), dc!(151.0)),
            (uc!(900.0), dc!(178.0)),
            (uc!(1000.0), dc!(208.0)),
        ]);

        assert_eq!(
            weight.userspace_to_designspace(uc!(250.0)).unwrap(),
            dc!(51.5)
        );
        assert_eq!(
            weight.designspace_to_userspace(dc!(138.0)).unwrap(),
            uc!(750.0)
        );
    }

    // #[test]
    // fn test_normalize_map() {
    //     let mut opsz = Axis::new("Optical Size".to_string(), Tag::from_be_bytes(*b"opsz"));
    //     opsz.min = Some(uc!(17.0));
    //     opsz.max = Some(uc!(18.0));
    //     opsz.default = Some(uc!(18.0));
    //     opsz.map = Some(vec![
    //         (uc!(17.0), dc!(17.0)),
    //         (uc!(17.99), dc!(17.1)),
    //         (uc!(18.0), dc!(18.0)),
    //     ]);
    //     assert_eq!(opsz.normalize_userspace_value(uc!(17.99)).unwrap(), -0.01);
    //     assert_eq!(opsz.normalize_designspace_value(dc!(17.1)).unwrap(), -0.9);
    // }
}
