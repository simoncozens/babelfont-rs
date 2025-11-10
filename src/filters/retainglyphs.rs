use crate::filters::FontFilter;

pub struct RetainGlyphs(Vec<String>);

impl RetainGlyphs {
    pub fn new(glyph_names: Vec<String>) -> Self {
        RetainGlyphs(glyph_names)
    }
}

impl FontFilter for RetainGlyphs {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        log::info!("Retaining glyphs: {:?}", self.0);
        let immutable_font = font.clone(); // Urgh
        for glyph in font.glyphs.iter_mut() {
            if !self.0.contains(&glyph.name) {
                continue;
            }
            // Check for components in layers
            for layer in glyph.layers.iter_mut() {
                let mut needs_decomposition = false;
                for shape in layer.shapes.iter_mut() {
                    if let crate::Shape::Component(comp) = shape {
                        if !self.0.contains(&comp.reference) {
                            needs_decomposition = true;
                        }
                    }
                }
                if needs_decomposition {
                    layer.decompose(&immutable_font);
                }
            }
        }
        // Retain only the specified glyphs
        font.glyphs.retain(|g| self.0.contains(&g.name));
        for (_group, members) in font.first_kern_groups.iter_mut() {
            members.retain(|g| self.0.contains(g));
        }
        for (_group, members) in font.second_kern_groups.iter_mut() {
            members.retain(|g| self.0.contains(g));
        }
        // Drop dead groups
        font.first_kern_groups
            .retain(|_group, members| !members.is_empty());
        font.second_kern_groups
            .retain(|_group, members| !members.is_empty());
        // Filter kerning
        for master in font.masters.iter_mut() {
            // XXX or groups!
            master
                .kerning
                .retain(|(left, right), _| self.0.contains(left) && self.0.contains(right));
        }
        // Filter features!
        Ok(())
    }
}
