use std::collections::{HashMap, HashSet};

use fea_rs_ast::{FeatureFile, GlyphContainer, GlyphName, LayoutVisitor, Statement};
use smol_str::SmolStr;

use crate::{BabelfontError, Font};

/// Given a font and a set of glyphs, return the closure of those glyphs under the layout features of the font.
pub fn close_layout(
    font: &Font,
    glyphs: HashSet<SmolStr>,
) -> Result<HashSet<SmolStr>, BabelfontError> {
    let features = font.features.to_fea();
    let font_glyphs: Vec<&str> = font.glyphs.iter().map(|g| g.name.as_str()).collect();
    let mut feature_file: FeatureFile =
        FeatureFile::new_from_fea(&features, Some(&font_glyphs), font.source.clone())
            .map_err(|e| crate::BabelfontError::FilterError(e.to_string()))?;
    // Rather annoyingly we may need to perform multiple rounds of this. Consider:
    // lookup A { sub b by c; } lookup A; feature foo { sub a by b; } foo; feature bar { sub b' lookup A; } bar;
    // The first round we would add b to the closure, but not c, because we skipped over lookup A as it didn't
    // contain any glyphs we cared about. We could avoid this by revisiting lookups during processing of
    // contextual substitutions, but that's tricky and this is a big hammer.
    let mut visitor = LayoutClosureVisitor::new(glyphs);

    let mut count = visitor.glyphs.len();
    let mut rounds = 0;
    loop {
        visitor.visit(&mut feature_file).map_err(|e| {
            crate::BabelfontError::FilterError(format!("Error during feature subsetting: {}", e))
        })?;
        rounds += 1;
        if visitor.glyphs.len() == count {
            break;
        }
        if rounds > 10 {
            return Err(BabelfontError::LayoutClosureError);
        }
        count = visitor.glyphs.len();
    }
    Ok(visitor.glyphs)
}

struct LayoutClosureVisitor {
    glyphs: HashSet<SmolStr>,
    original_class_definitions: HashMap<SmolStr, Vec<SmolStr>>,
}
impl LayoutClosureVisitor {
    fn new(glyphs: HashSet<SmolStr>) -> Self {
        Self {
            glyphs,
            original_class_definitions: HashMap::new(),
        }
    }

    fn expand_glyph_container(&self, gc: &GlyphContainer) -> Vec<SmolStr> {
        // Expand original glyphs recursively
        let mut todo = vec![gc.clone()];
        let mut original_glyphs = vec![];
        while let Some(container) = todo.pop() {
            match container {
                GlyphContainer::GlyphName(glyph_name) => {
                    original_glyphs.push(glyph_name.name.clone());
                }
                GlyphContainer::GlyphClassName(mut class_name) => {
                    if class_name.starts_with("@") {
                        class_name = class_name[1..].into();
                    }
                    if let Some(definition) = self.original_class_definitions.get(&class_name) {
                        for glyph in definition.iter().rev() {
                            todo.push(GlyphContainer::GlyphName(GlyphName::new(glyph)));
                        }
                    } else {
                        log::warn!(
                            "Warning: no definition found for glyph class {}",
                            class_name
                        );
                    }
                }
                GlyphContainer::GlyphClass(glyph_class) => {
                    for gc in glyph_class.glyphs.iter().rev() {
                        todo.push(gc.clone());
                    }
                }
                GlyphContainer::GlyphNameOrRange(name) => {
                    // I'm just going to treat it as a glyph name for now
                    original_glyphs.push(name.clone());
                }
                GlyphContainer::GlyphRange(range) => {
                    for glyph in range.glyphset() {
                        original_glyphs.push(glyph);
                    }
                }
            }
        }
        original_glyphs
    }

    fn contains(&self, gc: &GlyphContainer) -> bool {
        self.expand_glyph_container(gc)
            .iter()
            .any(|g| self.glyphs.contains(g.as_str()))
    }

    fn is_excluded_by_context(&self, prefix: &[GlyphContainer], suffix: &[GlyphContainer]) -> bool {
        prefix.iter().any(|gc| !self.contains(gc)) || suffix.iter().any(|gc| !self.contains(gc))
    }

    fn close_single_subst(&mut self, statement: &mut fea_rs_ast::SingleSubstStatement) {
        if self.is_excluded_by_context(&statement.prefix, &statement.suffix) {
            return;
        }
        // We have to go pairwise over glyph->replacement,
        // looking into class definitions as we do so.
        let mapping_from: Vec<SmolStr> = statement
            .glyphs
            .iter()
            .flat_map(|gc| self.expand_glyph_container(gc))
            .collect::<Vec<_>>();
        let mapping_to: Vec<SmolStr> = statement
            .replacement
            .iter()
            .flat_map(|gc| self.expand_glyph_container(gc))
            .collect::<Vec<_>>();
        let mapping = mapping_from.into_iter().zip(mapping_to);
        for (from, to) in mapping {
            if self.glyphs.contains(from.as_str()) {
                log::debug!(
                    "Adding glyph from SingleSubst substitution: {:?} -> {:?}",
                    from,
                    to
                );
                self.glyphs.insert(to.clone());
            }
        }
    }
    fn close_multiple_subst(&mut self, statement: &mut fea_rs_ast::MultipleSubstStatement) {
        if self.is_excluded_by_context(&statement.prefix, &statement.suffix) {
            return;
        }
        if self.contains(&statement.glyph) {
            for replacement in statement.replacement.iter() {
                for glyph in self.expand_glyph_container(replacement) {
                    log::debug!(
                        "Adding glyph from MultipleSubst substitution: {:?} -> {:?}",
                        statement.glyph,
                        glyph
                    );
                    self.glyphs.insert(glyph.clone());
                }
            }
        }
    }
    fn close_alternate_subst(&mut self, statement: &mut fea_rs_ast::AlternateSubstStatement) {
        if self.is_excluded_by_context(&statement.prefix, &statement.suffix) {
            return;
        }
        if self.contains(&statement.glyph) {
            let alternate = &statement.replacement;
            for glyph in self.expand_glyph_container(alternate) {
                log::debug!(
                    "Adding glyph from AlternateSubst substitution: {:?} -> {:?}",
                    statement.glyph,
                    glyph
                );
                self.glyphs.insert(glyph.clone());
            }
        }
    }
    fn close_ligature_subst(&mut self, statement: &mut fea_rs_ast::LigatureSubstStatement) {
        if self.is_excluded_by_context(&statement.prefix, &statement.suffix) {
            return;
        }
        if statement.glyphs.iter().all(|gc| self.contains(gc)) {
            for ligature in self.expand_glyph_container(&statement.replacement) {
                log::debug!(
                    "Adding glyph from LigatureSubst substitution: {:?} -> {:?}",
                    statement.glyphs,
                    ligature
                );
                self.glyphs.insert(ligature.clone());
            }
        }
    }
    fn close_reverse_chain_single_subst(
        &mut self,
        statement: &mut fea_rs_ast::ReverseChainSingleSubstStatement,
    ) {
        if self.is_excluded_by_context(&statement.prefix, &statement.suffix) {
            return;
        }
        if statement.glyphs.iter().all(|gc| self.contains(gc)) {
            for replacement in statement.replacements.iter() {
                for glyph in self.expand_glyph_container(replacement) {
                    log::debug!(
                        "Adding glyph from ReverseChainSubst substitution: {:?} -> {:?}",
                        statement.glyphs,
                        glyph
                    );
                    self.glyphs.insert(glyph.clone());
                }
            }
        }
    }

    fn close_glyph_class_definition(&mut self, statement: &mut fea_rs_ast::GlyphClassDefinition) {
        // Store the original class definition
        let name = statement.name.clone();

        let original_glyphs = statement
            .glyphs
            .glyphs
            .iter()
            .flat_map(|gc| self.expand_glyph_container(gc))
            .collect();
        self.original_class_definitions
            .insert(name.into(), original_glyphs);
    }
}

impl LayoutVisitor for LayoutClosureVisitor {
    fn depth_first(&self) -> bool {
        true
    }
    fn visit_statement(&mut self, statement: &mut Statement) -> bool {
        match statement {
            Statement::SingleSubst(single_subst_statement) => {
                self.close_single_subst(single_subst_statement)
            }
            Statement::MultipleSubst(multiple_subst_statement) => {
                self.close_multiple_subst(multiple_subst_statement)
            }
            Statement::AlternateSubst(alternate_subst_statement) => {
                self.close_alternate_subst(alternate_subst_statement)
            }
            Statement::LigatureSubst(ligature_subst_statement) => {
                self.close_ligature_subst(ligature_subst_statement)
            }
            Statement::ReverseChainSubst(reverse_chain_single_subst_statement) => {
                self.close_reverse_chain_single_subst(reverse_chain_single_subst_statement)
            }
            Statement::GlyphClassDefinition(glyph_class_definition) => {
                self.close_glyph_class_definition(glyph_class_definition)
            }
            _ => {}
        }
        true
    }
}

#[allow(clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Font, Glyph};
    use pretty_assertions::assert_eq;

    fn dummy_font_with_glyphs_and_features(glyph_names: Vec<&str>, feature_code: &str) -> Font {
        let mut font = Font::new();
        for name in glyph_names {
            font.glyphs.push(Glyph::new(name));
        }
        font.features = crate::Features::from_fea(feature_code);
        font
    }

    fn test_closure(font: Font, initial_glyphs: Vec<&str>, expected_glyphs: Vec<&str>) {
        let initial_glyphs_set: HashSet<SmolStr> =
            initial_glyphs.iter().map(|s| (*s).into()).collect();
        let result = close_layout(&font, initial_glyphs_set).expect("Layout closure failed");
        let mut result = result.into_iter().collect::<Vec<_>>();
        result.sort();
        let expected_glyphs: Vec<SmolStr> = expected_glyphs.iter().map(|s| (*s).into()).collect();
        assert_eq!(result, expected_glyphs);
    }

    #[test]
    fn test_closure_1() {
        test_closure(
            dummy_font_with_glyphs_and_features(
                vec!["a", "b", "c", "d", "e", "f"],
                "feature foo { sub a by d; sub b by e; } foo;\nfeature bar { sub c by f; } bar;\n",
            ),
            vec!["a", "c"],
            vec!["a", "c", "d", "f"],
        );
    }

    #[test]
    fn test_closure_2() {
        test_closure(
            dummy_font_with_glyphs_and_features(
                vec!["a", "b", "c", "d", "e", "f"],
                "feature foo { sub b a' by d; sub a by e; } foo;",
            ),
            vec!["a", "c"],
            vec!["a", "c", "e"], // d should not be included because we don't have the prefix glyph b
        );
    }

    #[test]
    fn test_closure_multiple_rounds() {
        test_closure(
            dummy_font_with_glyphs_and_features(
                vec!["a", "b", "c", "d", "e", "f"],
                "lookup A { sub b by c; } A; feature foo { sub a by b; } foo; feature bar { sub b' lookup A; } bar;",
            ),
            vec!["a"],
            vec!["a", "b", "c"],
        );
    }
}
