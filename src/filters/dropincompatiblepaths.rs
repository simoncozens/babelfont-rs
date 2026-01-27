use crate::{filters::FontFilter, Layer};

/// A filter that removes any incompatible paths across a font's layers
#[derive(Default)]
pub struct DropIncompatiblePaths;

impl DropIncompatiblePaths {
    /// Create a new DropIncompatiblePaths filter
    pub fn new() -> Self {
        DropIncompatiblePaths
    }
}

impl FontFilter for DropIncompatiblePaths {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        log::info!("Dropping incompatible paths from font");
        let mut todo_list = Vec::new();

        for glyph in font.glyphs.iter_mut() {
            let effective_layers: Vec<&Layer> = glyph
                .layers
                .iter()
                .filter(|layer| {
                    matches!(layer.master, crate::LayerType::DefaultForMaster(_))
                        || layer.location.is_some()
                })
                .collect();
            if effective_layers.len() < 2 {
                continue;
            }
            #[allow(clippy::unwrap_used)] // We know there's at least 2
            let (first, others) = effective_layers.split_first().unwrap();
            let first_path_count = first.paths().count();
            if others
                .iter()
                .any(|layer| layer.paths().count() != first_path_count)
            {
                log::info!(
                    "  Dropping paths for glyph '{}' due to incompatible number of paths",
                    glyph.name
                );
                for layer in glyph.layers.iter_mut() {
                    layer.shapes.retain(|shape| !shape.is_path());
                }
                break;
            }
            // Now we know the path counts are identical, check each path's commands
            let path_count = first.paths().count();
            for path_index in 0..path_count {
                #[allow(clippy::unwrap_used)]
                // We know this is within bounds and path counts are identical
                let first_path = first.paths().nth(path_index).unwrap();
                for other in others {
                    #[allow(clippy::unwrap_used)]
                    let other_path = other.paths().nth(path_index).unwrap();
                    if first_path.nodes.len() != other_path.nodes.len() {
                        log::info!(
                            "  Dropping paths for glyph '{}' due to incompatible path node counts",
                            glyph.name
                        );
                        todo_list.push((glyph.name.clone(), path_index));
                        break;
                    }
                    for (node_a, node_b) in first_path.nodes.iter().zip(other_path.nodes.iter()) {
                        if node_a.nodetype != node_b.nodetype {
                            log::info!(
                                "  Dropping paths for glyph '{}' due to incompatible path node kinds",
                                glyph.name
                            );
                            todo_list.push((glyph.name.clone(), path_index));
                            break;
                        }
                    }
                }
            }
        }
        // Order the todo-list with glyphs first, then path indices descending
        todo_list.sort_by(|a, b| {
            let glyph_order = a.0.cmp(&b.0);
            if glyph_order == std::cmp::Ordering::Equal {
                b.1.cmp(&a.1)
            } else {
                glyph_order
            }
        });
        // Now process the todo-list
        for (glyph_name, path_index) in todo_list {
            if let Some(glyph) = font.glyphs.iter_mut().find(|g| g.name == glyph_name) {
                for layer in glyph.layers.iter_mut() {
                    let mut path_counter = 0;
                    layer.shapes.retain(|shape| {
                        if shape.is_path() {
                            if path_counter == path_index {
                                path_counter += 1;
                                return false;
                            } else {
                                path_counter += 1;
                            }
                        }
                        true
                    });
                }
            }
        }
        Ok(())
    }

    fn from_str(_s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(DropIncompatiblePaths::new())
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("dropincompatiblepaths")
            .long("drop-incompatible-paths")
            .help("Remove incompatible paths from the font's layers")
            .action(clap::ArgAction::SetTrue)
    }
}
