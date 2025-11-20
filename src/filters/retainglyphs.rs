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
            master.kerning.retain(|(left, right), _| {
                // Because we removed all the dead groups, any groups still refer to things we care about
                (self.0.contains(left)
                    || (left.starts_with('@') && font.first_kern_groups.contains_key(&left[1..])))
                    && (self.0.contains(right)
                        || (right.starts_with('@')
                            && font.second_kern_groups.contains_key(&right[1..])))
            });
        }
        // Filter features!
        

        // Filter masters - remove any masters which were just sparse
        font.masters.retain(|master| {
            font.glyphs.iter().any(|glyph| {
                glyph.layers.iter().any(|layer| {
                    layer.master == crate::LayerType::DefaultForMaster(master.id.clone())
                })
            })
        });
        Ok(())
    }
}
