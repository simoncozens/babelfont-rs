use crate::common::{Color, Position};

#[derive(Debug, Default, Clone)]
pub struct Guide {
    pub pos: Position,
    pub name: Option<String>,
    pub color: Option<Color>,
    // lib
}

impl Guide {
    pub fn new() -> Self {
        Guide::default()
    }
}

#[cfg(feature = "ufo")]
impl From<&norad::Guideline> for Guide {
    fn from(g: &norad::Guideline) -> Self {
        let mut out = Guide::new();
        out.name = g.name.as_ref().map(|x| x.to_string());
        out.color = g.color.as_ref().map(|x| x.into());
        match g.line {
            norad::Line::Angle { x, y, degrees } => {
                out.pos = Position {
                    x: x as f32,
                    y: y as f32,
                    angle: degrees as f32,
                }
            }
            norad::Line::Horizontal(y) => {
                out.pos = Position {
                    x: 0.0,
                    y: y as f32,
                    angle: 0.0,
                }
            }
            norad::Line::Vertical(x) => {
                out.pos = Position {
                    y: 0.0,
                    x: x as f32,
                    angle: 90.0,
                }
            }
        };
        out
    }
}

#[cfg(feature = "glyphs")]
mod glyphs {
    use glyphslib::{common::GuideAlignment, glyphs3::Guide as G3Guide};

    use super::*;
    impl From<&G3Guide> for Guide {
        fn from(val: &G3Guide) -> Self {
            Guide {
                pos: Position {
                    x: val.pos.0,
                    y: val.pos.1,
                    angle: val.angle,
                },
                name: None,
                color: None,
            }
        }
    }

    impl From<&Guide> for G3Guide {
        fn from(val: &Guide) -> Self {
            G3Guide {
                pos: (val.pos.x, val.pos.y),
                angle: val.pos.angle,
                alignment: GuideAlignment::Left,
                locked: false,
                scale: (1.0, 1.0),
            }
        }
    }
}
