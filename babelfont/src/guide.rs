use crate::common::{Color, Position};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "types", typeshare::typeshare)]
/// A guideline in the font, whether at master or layer level
pub struct Guide {
    /// Position of the guideline
    pub pos: Position,
    /// Optional name of the guideline
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Optional color of the guideline
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    /// Format-specific data
    #[serde(
        default,
        skip_serializing_if = "crate::common::FormatSpecific::is_empty"
    )]
    #[typeshare(python(type = "Dict[str, Any]"))]
    #[typeshare(typescript(type = "Record<string, any>"))]
    pub format_specific: crate::common::FormatSpecific,
}

impl Guide {
    /// Create a new, empty Guide
    pub fn new() -> Self {
        Guide::default()
    }
}

#[cfg(feature = "ufo")]
mod ufo {
    use crate::{
        convertors::ufo::{KEY_IDENTIFIER, KEY_LIB, KEY_ORIGINAL_GUIDE},
        BabelfontError,
    };

    use super::*;
    impl From<&norad::Guideline> for Guide {
        fn from(g: &norad::Guideline) -> Self {
            let mut out = Guide::new();
            out.name = g.name.as_ref().map(|x| x.to_string());
            out.color = g.color.as_ref().map(|x| x.into());
            if let Some(lib) = g.lib() {
                out.format_specific.insert(
                    KEY_LIB.to_string(),
                    serde_json::to_value(lib).unwrap_or_default(),
                );
            }
            if let Some(identifier) = g.identifier() {
                out.format_specific.insert(
                    KEY_IDENTIFIER.to_string(),
                    serde_json::to_value(identifier).unwrap_or_default(),
                );
            }
            match g.line {
                norad::Line::Angle { x, y, degrees } => {
                    out.pos = Position {
                        x: x as f32,
                        y: y as f32,
                        angle: degrees as f32,
                    };
                    out.format_specific.insert(
                        KEY_ORIGINAL_GUIDE.to_string(),
                        serde_json::to_value("angled").unwrap_or_default(),
                    );
                }
                norad::Line::Horizontal(y) => {
                    out.pos = Position {
                        x: 0.0,
                        y: y as f32,
                        angle: 0.0,
                    };
                    out.format_specific.insert(
                        KEY_ORIGINAL_GUIDE.to_string(),
                        serde_json::to_value("horizontal").unwrap_or_default(),
                    );
                }
                norad::Line::Vertical(x) => {
                    out.pos = Position {
                        y: 0.0,
                        x: x as f32,
                        angle: 90.0,
                    };
                    out.format_specific.insert(
                        KEY_ORIGINAL_GUIDE.to_string(),
                        serde_json::to_value("vertical").unwrap_or_default(),
                    );
                }
            };
            out
        }
    }
    impl TryFrom<&Guide> for norad::Guideline {
        type Error = BabelfontError;

        fn try_from(g: &Guide) -> Result<Self, BabelfontError> {
            let name = g.name.as_ref().map(|x| norad::Name::new(x)).transpose()?;
            let color = g.color.as_ref().map(|x| x.try_into()).transpose()?;
            let was_angled = g
                .format_specific
                .get(KEY_ORIGINAL_GUIDE)
                .and_then(|x| serde_json::from_value(x.clone()).ok())
                .and_then(|s: String| match s.as_str() {
                    "horizontal" => Some("horizontal"),
                    "vertical" => Some("vertical"),
                    "angled" => Some("angled"),
                    _ => None,
                })
                == Some("angled");
            let line = match (g.pos.x, g.pos.y, g.pos.angle, was_angled) {
                (_, y, 0.0, false) => norad::Line::Horizontal(y as f64),
                (x, _, 90.0, false) => norad::Line::Vertical(x as f64),
                (x, y, angle, _) => norad::Line::Angle {
                    x: x as f64,
                    y: y as f64,
                    degrees: angle as f64,
                },
            };
            let identifier = g
                .format_specific
                .get(KEY_IDENTIFIER)
                .and_then(|x| serde_json::from_value(x.clone()).ok());
            let mut guide = norad::Guideline::new(line, name, color, identifier);

            if let Some(lib) = g
                .format_specific
                .get(KEY_LIB)
                .and_then(|x| serde_json::from_value(x.clone()).ok())
            {
                guide.replace_lib(lib);
            }
            Ok(guide)
        }
    }
}

#[cfg(feature = "glyphs")]
mod glyphs {
    use glyphslib::glyphs3::Guide as G3Guide;

    use super::*;
    impl From<&G3Guide> for Guide {
        fn from(val: &G3Guide) -> Self {
            let mut format_specific = crate::common::FormatSpecific::default();
            macro_rules! extract_and_insert {
                ($key:expr, $getter:expr) => {
                    format_specific.insert(
                        $key.to_string(),
                        serde_json::to_value($getter(val)).unwrap_or_default(),
                    );
                };
            }
            extract_and_insert!("filter", |v: &G3Guide| v.filter.clone());
            extract_and_insert!("grid", |v: &G3Guide| v.grid);
            extract_and_insert!("length", |v: &G3Guide| v.length);
            extract_and_insert!("locked", |v: &G3Guide| v.locked);
            extract_and_insert!("lockAngle", |v: &G3Guide| v.lock_angle);
            extract_and_insert!("orientation", |v: &G3Guide| v.orientation);
            extract_and_insert!("showMeasurement", |v: &G3Guide| v.show_measurement);
            extract_and_insert!("size", |v: &G3Guide| v.size);
            extract_and_insert!("type", |v: &G3Guide| v.guide_type);
            extract_and_insert!("userData", |v: &G3Guide| v.user_data.clone());

            Guide {
                pos: Position {
                    x: val.pos.0,
                    y: val.pos.1,
                    angle: val.angle,
                },
                name: if val.name.is_empty() {
                    None
                } else {
                    Some(val.name.clone())
                },
                color: None,
                format_specific,
            }
        }
    }

    impl From<&Guide> for G3Guide {
        fn from(val: &Guide) -> Self {
            macro_rules! extract_format_specific {
                ($key:expr, $ty:ty) => {
                    val.format_specific
                        .get($key)
                        .and_then(|x| serde_json::from_value(x.clone()).ok())
                        .unwrap_or_default()
                };
            }
            G3Guide {
                pos: (val.pos.x, val.pos.y),
                angle: val.pos.angle,
                name: val.name.clone().unwrap_or_default(),
                grid: extract_format_specific!("grid", bool),
                length: extract_format_specific!("length", bool),
                locked: extract_format_specific!("locked", bool),
                lock_angle: extract_format_specific!("lockAngle", bool),
                orientation: extract_format_specific!("orientation", Orientation),
                show_measurement: extract_format_specific!("showMeasurement", bool),
                size: extract_format_specific!("size", (f32, f32)),
                guide_type: extract_format_specific!("type", String),
                filter: extract_format_specific!("filter", String),
                user_data: extract_format_specific!("userData", String),
            }
        }
    }
}

#[cfg(feature = "fontra")]
mod fontra {
    use super::*;
    use crate::convertors::fontra::Guideline as FontraGuideline;

    impl From<&Guide> for FontraGuideline {
        fn from(val: &Guide) -> Self {
            FontraGuideline {
                name: val.name.clone(),
                x: val.pos.x,
                y: val.pos.y,
                angle: val.pos.angle,
                locked: false,
            }
        }
    }
}
