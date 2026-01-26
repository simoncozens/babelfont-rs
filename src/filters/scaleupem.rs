use crate::filters::FontFilter;

/// A filter that scales the UPEM of a font and adjusts all relevant metrics accordingly
pub struct ScaleUpem(u16);

impl ScaleUpem {
    /// Create a new ScaleUpem filter
    pub fn new(new_upem: u16) -> Self {
        ScaleUpem(new_upem)
    }
}

impl FontFilter for ScaleUpem {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        let old_upem = font.upm as f64;
        let newupem = self.0 as f64;
        let scale_factor = newupem / old_upem;
        log::info!(
            "Scaling UPEM from {} to {}, scale factor {}",
            font.upm,
            self.0,
            scale_factor
        );

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
                            // Just scale any translations in the transform
                            comp.transform.translation.0 *= scale_factor;
                            comp.transform.translation.1 *= scale_factor;
                        }
                    }
                }
            }
        }
        font.upm = self.0;
        Ok(())
    }

    fn from_str(s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        let new_upem: u16 = s.parse().map_err(|_| {
            crate::BabelfontError::FilterError(format!("Invalid UPEM value: {}", s))
        })?;
        Ok(ScaleUpem::new(new_upem))
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("scaleupem")
            .long("scaleupem")
            .help("Scale the font's units per em to the specified value")
            .value_name("UPEM")
            .action(clap::ArgAction::Append)
    }
}
