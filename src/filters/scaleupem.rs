use crate::filters::FontFilter;

pub struct ScaleUpem(u16);

impl ScaleUpem {
    pub fn new(new_upem: u16) -> Self {
        ScaleUpem(new_upem)
    }
}

impl FontFilter for ScaleUpem {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        log::info!("Scaling UPEM from {} to {}", font.upm, self.0);
        let old_upem = font.upm as f64;
        let newupem = self.0 as f64;
        let scale_factor = newupem / old_upem;

        // Scale all metrics
        for master in font.masters.iter_mut() {
            for (_metric_type, value) in master.metrics.iter_mut() {
                *value = (*value as f64 * scale_factor) as i32;
            }
            // Scale all guides
            for guide in master.guides.iter_mut() {
                guide.pos.x *= scale_factor as f32;
                guide.pos.y *= scale_factor as f32;
            }
            // Scale kerning
            for ((_left, _right), value) in master.kerning.iter_mut() {
                *value = (*value as f64 * scale_factor) as i16;
            }
        }

        // Scale all glyphs
        for glyph in font.glyphs.iter_mut() {
            for layer in glyph.layers.iter_mut() {
                // Scale width
                layer.width *= scale_factor as f32;

                // Scale shapes
                for shape in layer.shapes.iter_mut() {
                    match shape {
                        crate::Shape::Path(path) => {
                            for point in path.nodes.iter_mut() {
                                point.x *= scale_factor;
                                point.y *= scale_factor;
                            }
                        }
                        crate::Shape::Component(comp) => {
                            comp.transform = comp.transform.pre_scale(scale_factor);
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
