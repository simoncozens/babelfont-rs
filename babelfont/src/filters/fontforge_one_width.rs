//! The advance a FontForge source's glyphs share, which decides whether FontForge
//! exports the font as fixed pitch.

use crate::{Font, Master, Shape};
use std::collections::HashSet;

/// `CIDOneWidth` in FontForge's `splinesave.c` up to tag 20230101: the advance shared
/// by every glyph FontForge would output, leaving out `.null`, `nonmarkingreturn` and
/// a `.notdef` with no contours. `None` when two advances differ or no glyph counts.
/// From tag 20251009 on, `SFOneWidth` also leaves out zero-width glyphs.
pub(crate) fn one_width(font: &Font, master: &Master) -> Option<i32> {
    let referenced: HashSet<&str> = font
        .glyphs
        .iter()
        .flat_map(|glyph| glyph.layers.iter())
        .flat_map(|layer| layer.shapes.iter())
        .filter_map(|shape| match shape {
            Shape::Component(component) => Some(component.reference.as_str()),
            Shape::Path(_) => None,
        })
        .collect();
    let mut width = None;
    for glyph in font.glyphs.iter() {
        let Some(layer) = font.master_layer_for(&glyph.name, master) else {
            continue;
        };
        let has_contours = layer.shapes.iter().any(|s| matches!(s, Shape::Path(_)));
        match glyph.name.as_str() {
            ".null" | "nonmarkingreturn" => continue,
            ".notdef" if !has_contours => continue,
            _ => {}
        }
        // `SCWorthOutputting`: it draws something, its width was set, it carries an
        // anchor or another glyph uses it as a component.
        let width_set = glyph
            .format_specific
            .get("sfd.width_set")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let worth_outputting = draws_something(font, master, &glyph.name, 0)
            || width_set
            || !layer.anchors.is_empty()
            || referenced.contains(glyph.name.as_str());
        if !worth_outputting {
            continue;
        }
        let advance = layer.width.round() as i32;
        match width {
            None => width = Some(advance),
            Some(w) if w != advance => return None,
            Some(_) => {}
        }
    }
    width
}

/// `SCDrawsSomething`: the glyph has a contour, or a component that brings one.
fn draws_something(font: &Font, master: &Master, name: &str, depth: usize) -> bool {
    const MAX_REFERENCE_DEPTH: usize = 8;
    let Some(layer) = font.master_layer_for(name, master) else {
        return false;
    };
    layer.shapes.iter().any(|shape| match shape {
        Shape::Path(_) => true,
        Shape::Component(component) => {
            depth < MAX_REFERENCE_DEPTH
                && draws_something(font, master, &component.reference, depth + 1)
        }
    })
}
