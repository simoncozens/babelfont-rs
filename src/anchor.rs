#[derive(Debug, Clone)]
pub struct Anchor {
    pub x: f32,
    pub y: f32,
    pub name: String,
}

#[cfg(feature = "ufo")]
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
