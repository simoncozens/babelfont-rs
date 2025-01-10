use std::ops::{Deref, DerefMut};

use crate::{
    common::{Direction, FormatSpecific},
    layer::Layer,
};

#[derive(Debug, Clone)]
pub struct GlyphList(pub Vec<Glyph>);
impl GlyphList {
    pub fn get(&self, g: &str) -> Option<&Glyph> {
        self.0.iter().find(|&glyph| glyph.name == g)
    }
    pub fn get_mut(&mut self, g: &str) -> Option<&mut Glyph> {
        self.0.iter_mut().find(|glyph| glyph.name == g)
    }

    pub fn get_by_index(&self, id: usize) -> Option<&Glyph> {
        self.0.get(id)
    }
    pub fn get_by_index_mut(&mut self, id: usize) -> Option<&mut Glyph> {
        self.0.get_mut(id)
    }
}

impl Deref for GlyphList {
    type Target = Vec<Glyph>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for GlyphList {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(Debug, Clone)]
pub enum GlyphCategory {
    Base,
    Mark,
    Unknown,
    Ligature,
}

#[derive(Debug, Clone)]
pub struct Glyph {
    pub name: String,
    pub production_name: Option<String>,
    pub category: GlyphCategory,
    pub codepoints: Vec<u32>,
    pub layers: Vec<Layer>,
    pub exported: bool,
    pub direction: Option<Direction>,
    pub formatspecific: FormatSpecific,
}

impl Glyph {
    pub fn get_layer(&self, id: &str) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id.as_deref() == Some(id))
    }
    pub fn get_layer_mut(&mut self, id: &str) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.id.as_deref() == Some(id))
    }
}

#[cfg(feature = "glyphs")]
mod glyphs {
    use std::collections::BTreeMap;

    use super::*;
    use glyphslib::glyphs3::Glyph as G3Glyph;

    impl From<&G3Glyph> for Glyph {
        fn from(val: &G3Glyph) -> Self {
            Glyph {
                name: val.name.clone(),
                production_name: val.production.clone(),
                category: GlyphCategory::Unknown, // XXX
                codepoints: val.unicode.clone(),
                layers: val.layers.iter().map(Into::into).collect(),
                exported: val.export,
                direction: None,
                formatspecific: Default::default(), // XXX
            }
        }
    }

    impl From<&Glyph> for G3Glyph {
        fn from(val: &Glyph) -> Self {
            G3Glyph {
                name: val.name.clone(),
                production: val.production_name.clone(),
                unicode: val.codepoints.clone(),
                layers: val.layers.iter().map(Into::into).collect(),
                export: val.exported,
                case: val.formatspecific.get_string("case"),
                category: val.formatspecific.get_optionstring("category"),
                direction: val.formatspecific.get_optionstring("kern_direction"),
                kern_bottom: val.formatspecific.get_optionstring("kern_bottom"),
                kern_left: val.formatspecific.get_optionstring("kern_left"),
                kern_right: val.formatspecific.get_optionstring("kern_right"),
                kern_top: val.formatspecific.get_optionstring("kern_top"),
                last_change: val.formatspecific.get_optionstring("last_change"),
                locked: val.formatspecific.get_bool("locked"),
                metric_bottom: val.formatspecific.get_optionstring("metric_bottom"),
                metric_left: val.formatspecific.get_optionstring("metric_left"),
                metric_right: val.formatspecific.get_optionstring("metric_right"),
                metric_top: val.formatspecific.get_optionstring("metric_top"),
                metric_vert_width: val.formatspecific.get_optionstring("metric_vert_width"),
                metric_width: val.formatspecific.get_optionstring("metric_width"),
                note: val.formatspecific.get_string("note"),
                smart_component_settings: vec![], // XXX
                script: val.formatspecific.get_optionstring("script"),
                subcategory: val.formatspecific.get_optionstring("subcategory"),
                tags: val
                    .formatspecific
                    .get("tags")
                    .and_then(|x| x.as_array())
                    .map(|x| {
                        x.iter()
                            .filter_map(|x| x.as_str())
                            .map(|x| x.to_string())
                            .collect()
                    })
                    .unwrap_or_default(),
                user_data: BTreeMap::new(), // Plist<->JSON magic required here
                color: None,
            }
        }
    }
}
