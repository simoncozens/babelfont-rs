use std::collections::HashMap;

use fea_rs_ast::{GlyphContainer, GlyphName};
use smol_str::SmolStr;

use crate::{filters::glyphs3predicates::resolve_glyphs_predicate_class, Font};

/// Expand a glyph container into a flat list of glyph names.
///
/// Named classes (`@foo`) are looked up in `class_definitions`, whose keys do
/// not include the leading `@`. If `class_bindings` is given, a class name is
/// first mapped through it (used when classes have been renamed) before the
/// lookup. Classes which cannot be found are skipped with a warning.
/// CIDs are not supported and expand to nothing.
pub(crate) fn expand_glyph_container(
    font: &Font,
    gc: &GlyphContainer,
    class_definitions: &HashMap<SmolStr, Vec<SmolStr>>,
    class_bindings: Option<&HashMap<SmolStr, SmolStr>>,
) -> Vec<SmolStr> {
    let mut todo = vec![gc.clone()];
    let mut glyphs = vec![];
    while let Some(container) = todo.pop() {
        match container {
            GlyphContainer::GlyphName(glyph_name) => glyphs.push(glyph_name.name.clone()),
            GlyphContainer::GlyphClassName(class_name) => {
                let class_name: SmolStr =
                    class_name.strip_prefix('@').unwrap_or(&class_name).into();
                let effective = class_bindings
                    .and_then(|b| b.get(&class_name))
                    .cloned()
                    .unwrap_or(class_name);
                if let Some(definition) = class_definitions.get(&effective) {
                    todo.extend(
                        definition
                            .iter()
                            .rev()
                            .map(|g| GlyphContainer::GlyphName(GlyphName::new(g))),
                    );
                } else {
                    log::warn!("No definition found for glyph class {}", effective);
                }
            }
            GlyphContainer::GlyphClass(glyph_class) => {
                todo.extend(glyph_class.glyphs.iter().rev().cloned());
            }
            // Treated as a plain glyph name
            GlyphContainer::GlyphNameOrRange(name) => glyphs.push(name.clone()),
            GlyphContainer::GlyphRange(range) => glyphs.extend(range.glyphset()),
            GlyphContainer::GlyphsPredicateClass(pc) => {
                glyphs.extend(resolve_glyphs_predicate_class(font, &pc));
            }
            GlyphContainer::Cid(_) | GlyphContainer::CidRange(_) => {}
        }
    }
    glyphs
}
