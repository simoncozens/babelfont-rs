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

/// Pair the source and target glyphs of a single substitution.
///
/// A single target glyph applies to every source glyph, as in
/// `sub [a b c] by x;`. Otherwise both sides are paired by position and must
/// have the same length. Returns `None` when the lengths don't match.
pub(crate) fn single_subst_pairs(
    from: Vec<SmolStr>,
    to: Vec<SmolStr>,
) -> Option<Vec<(SmolStr, SmolStr)>> {
    if to.len() == 1 {
        let target = &to[0];
        return Some(from.into_iter().map(|f| (f, target.clone())).collect());
    }
    if from.len() != to.len() {
        return None;
    }
    Some(from.into_iter().zip(to).collect())
}

#[cfg(test)]
mod tests {
    use super::single_subst_pairs;
    use smol_str::SmolStr;

    fn names(glyphs: &[&str]) -> Vec<SmolStr> {
        glyphs.iter().map(SmolStr::new).collect()
    }

    fn pairs(from: &[&str], to: &[&str]) -> Option<Vec<(String, String)>> {
        single_subst_pairs(names(from), names(to)).map(|pairs| {
            pairs
                .into_iter()
                .map(|(f, t)| (f.to_string(), t.to_string()))
                .collect()
        })
    }

    fn owned(pairs: &[(&str, &str)]) -> Option<Vec<(String, String)>> {
        Some(
            pairs
                .iter()
                .map(|(f, t)| (f.to_string(), t.to_string()))
                .collect(),
        )
    }

    #[test]
    fn single_target_applies_to_every_source() {
        assert_eq!(
            pairs(&["a", "b", "c"], &["x"]),
            owned(&[("a", "x"), ("b", "x"), ("c", "x")])
        );
    }

    #[test]
    fn equal_lengths_pair_by_position() {
        assert_eq!(
            pairs(&["a", "b"], &["x", "y"]),
            owned(&[("a", "x"), ("b", "y")])
        );
    }

    #[test]
    fn mismatched_lengths_are_rejected() {
        assert_eq!(pairs(&["a", "b", "c"], &["x", "y"]), None);
    }
}
