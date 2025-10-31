use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Anchor {
    pub x: f64,
    pub y: f64,
    pub name: String,
}

#[cfg(feature = "ufo")]
mod ufo {
    use crate::BabelfontError;

    use super::*;
    impl From<&norad::Anchor> for Anchor {
        fn from(a: &norad::Anchor) -> Self {
            Anchor {
                x: a.x,
                y: a.y,
                name: a
                    .name
                    .as_ref()
                    .map(|x| x.to_string())
                    .unwrap_or_else(|| "<Unnamed anchor>".to_string()),
            }
        }
    }

    impl TryFrom<&Anchor> for norad::Anchor {
        type Error = BabelfontError;

        fn try_from(a: &Anchor) -> Result<Self, BabelfontError> {
            Ok(norad::Anchor::new(
                a.x,
                a.y,
                Some(norad::Name::new(&a.name)?),
                None,
                None,
                None,
            ))
        }
    }
}

#[cfg(feature = "glyphs")]
mod glyphs {
    use super::*;

    impl From<&glyphslib::glyphs3::Anchor> for Anchor {
        fn from(val: &glyphslib::glyphs3::Anchor) -> Self {
            Anchor {
                name: val.name.clone(),
                x: val.pos.0 as f64,
                y: val.pos.1 as f64,
            }
        }
    }

    impl From<&Anchor> for glyphslib::glyphs3::Anchor {
        fn from(val: &Anchor) -> Self {
            glyphslib::glyphs3::Anchor {
                name: val.name.clone(),
                pos: (val.x as f32, val.y as f32),
            }
        }
    }
}

#[cfg(feature = "fontra")]
mod fontra {
    use super::*;
    use crate::convertors::fontra;

    impl From<&fontra::Anchor> for Anchor {
        fn from(val: &fontra::Anchor) -> Self {
            Anchor {
                name: val.name.clone(),
                x: val.x as f64,
                y: val.y as f64,
            }
        }
    }

    impl From<&Anchor> for fontra::Anchor {
        fn from(val: &Anchor) -> Self {
            fontra::Anchor {
                name: val.name.clone(),
                x: val.x as f32,
                y: val.y as f32,
            }
        }
    }
}
