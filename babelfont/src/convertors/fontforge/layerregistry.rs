use std::collections::HashMap;

use crate::{Font, Layer, LayerType};

#[derive(Debug, Default)]
pub(crate) struct LayerRegistry {
    pub(crate) layer_count: usize,
    pub(crate) defs: Vec<(usize, bool, String, usize)>,
    pub(crate) extras: HashMap<String, usize>,
}

impl LayerRegistry {
    pub(crate) fn from_font(font: &Font, default_master_id: &str) -> Self {
        let mut extra_quadratic: HashMap<String, bool> = HashMap::new();
        let mut defs: Vec<(usize, bool, String, usize)> = font
            .format_specific
            .get("sfd.layer_defs")
            .and_then(|v| v.as_array())
            .map(|defs| {
                defs.iter()
                    .filter_map(|entry| {
                        let obj = entry.as_object()?;
                        let index = obj.get("index")?.as_u64()? as usize;
                        let name = obj.get("name")?.as_str()?.to_string();
                        let is_quadratic = obj
                            .get("is_quadratic")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        let flags = obj.get("flags").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                        Some((index, is_quadratic, name, flags))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut extras: HashMap<String, usize> = defs
            .iter()
            .filter(|(_, _, name, _)| {
                !name.eq_ignore_ascii_case("Back") && !name.eq_ignore_ascii_case("Fore")
            })
            .map(|(idx, _, name, _)| (name.clone(), *idx))
            .collect();
        let mut next_idx = defs
            .iter()
            .map(|(idx, _, _, _)| *idx)
            .max()
            .map(|v| v + 1)
            .unwrap_or(2);

        for glyph in font.glyphs.iter() {
            for layer in &glyph.layers {
                if Self::is_background_layer(layer)
                    || Self::is_foreground_layer(layer, default_master_id)
                {
                    continue;
                }
                let key = Self::layer_key(layer);
                if let std::collections::hash_map::Entry::Vacant(v) = extras.entry(key) {
                    extra_quadratic.insert(v.key().clone(), super::layer_is_quadratic(layer));
                    v.insert(next_idx);
                    next_idx += 1;
                }
            }
        }

        if defs.is_empty() {
            defs = vec![
                (0, false, "Back".to_string(), 1),
                (1, false, "Fore".to_string(), 0),
            ];
        }

        let mut extra_pairs: Vec<(&String, &usize)> = extras.iter().collect();
        extra_pairs.sort_by_key(|(_, ix)| **ix);
        for (key, ix) in extra_pairs {
            if !defs
                .iter()
                .any(|(existing_idx, _, _, _)| existing_idx == ix)
            {
                defs.push((
                    *ix,
                    extra_quadratic.get(key).copied().unwrap_or(false),
                    key.clone(),
                    0,
                ));
            }
        }
        defs.sort_by_key(|(idx, _, _, _)| *idx);

        Self {
            layer_count: defs.len(),
            defs,
            extras,
        }
    }

    pub(crate) fn is_background_layer(layer: &Layer) -> bool {
        layer.is_background
            || layer
                .name
                .as_deref()
                .map(|n| n.eq_ignore_ascii_case("Back"))
                .unwrap_or(false)
    }

    pub(crate) fn is_foreground_layer(layer: &Layer, default_master_id: &str) -> bool {
        matches!(&layer.master, LayerType::DefaultForMaster(id) if id == default_master_id)
            || layer
                .name
                .as_deref()
                .map(|n| n.eq_ignore_ascii_case("Fore"))
                .unwrap_or(false)
    }

    fn layer_key(layer: &Layer) -> String {
        layer
            .name
            .clone()
            .or_else(|| layer.id.clone())
            .unwrap_or_else(|| "Layer".to_string())
    }

    pub(crate) fn index_for(&self, layer: &Layer, default_master_id: &str) -> usize {
        if Self::is_background_layer(layer) {
            0
        } else if Self::is_foreground_layer(layer, default_master_id) {
            1
        } else {
            self.extras
                .get(&Self::layer_key(layer))
                .copied()
                .unwrap_or(1)
        }
    }
}
