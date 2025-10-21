#[derive(Debug, Clone)]
pub struct Anchor {
    pub x: f32,
    pub y: f32,
    pub name: String,
}

#[cfg(feature = "ufo")]
mod ufo {
    use crate::BabelfontError;

    use super::*;
    impl From<&norad::Anchor> for Anchor {
        fn from(a: &norad::Anchor) -> Self {
            Anchor {
                x: a.x as f32,
                y: a.y as f32,
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
                a.x as f64,
                a.y as f64,
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
                x: val.pos.0,
                y: val.pos.1,
            }
        }
    }

    impl From<&Anchor> for glyphslib::glyphs3::Anchor {
        fn from(val: &Anchor) -> Self {
            glyphslib::glyphs3::Anchor {
                name: val.name.clone(),
                pos: (val.x, val.y),
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
                x: val.x,
                y: val.y,
            }
        }
    }

    impl From<&Anchor> for fontra::Anchor {
        fn from(val: &Anchor) -> Self {
            fontra::Anchor {
                name: val.name.clone(),
                x: val.x,
                y: val.y,
            }
        }
    }
}
