use std::collections::HashMap;

use fea_rs_ast::{GlyphContainer, GlyphName, LayoutVisitor, Statement, SubOrPos};
use smol_str::SmolStr;

/// A layout visitor that renames glyphs in the font
pub struct GlyphRenamerVisitor {
    mapping: HashMap<SmolStr, SmolStr>,
}
impl GlyphRenamerVisitor {
    pub fn new(mapping: HashMap<SmolStr, SmolStr>) -> Self {
        Self { mapping }
    }

    fn rename(&self, s: &SmolStr) -> SmolStr {
        self.mapping.get(s).cloned().unwrap_or_else(|| s.clone())
    }

    fn visit_glyph_container(&self, gc: &mut GlyphContainer) {
        match gc {
            GlyphContainer::GlyphName(ref mut glyph_name) => {
                *glyph_name = GlyphName::new(&self.rename(&glyph_name.name));
            }
            GlyphContainer::GlyphClassName(_) => {
                // Keep it
            }
            GlyphContainer::GlyphClass(ref mut glyph_class) => {
                // Visit recursively
                for gc in glyph_class.glyphs.iter_mut() {
                    self.visit_glyph_container(gc);
                }
            }
            GlyphContainer::GlyphNameOrRange(ref mut name) => {
                // I'm just going to treat it as a glyph name for now
                *name = self.rename(name);
            }
            GlyphContainer::GlyphRange(ref mut range) => {
                range.start = self.rename(&range.start);
                range.end = self.rename(&range.end);
            }
        }
    }

    fn visit_single_subst(&self, statement: &mut fea_rs_ast::SingleSubstStatement) {
        for vec_container in [
            statement.prefix.iter_mut(),
            statement.suffix.iter_mut(),
            statement.glyphs.iter_mut(),
            statement.replacement.iter_mut(),
        ] {
            for container in vec_container {
                self.visit_glyph_container(container);
            }
        }
    }
    fn visit_multiple_subst(&self, statement: &mut fea_rs_ast::MultipleSubstStatement) {
        self.visit_glyph_container(&mut statement.glyph);
        for vec_container in [
            statement.replacement.iter_mut(),
            statement.prefix.iter_mut(),
            statement.suffix.iter_mut(),
        ] {
            for container in vec_container {
                self.visit_glyph_container(container);
            }
        }
    }
    fn visit_alternate_subst(&self, statement: &mut fea_rs_ast::AlternateSubstStatement) {
        self.visit_glyph_container(&mut statement.glyph);
        self.visit_glyph_container(&mut statement.replacement);
        for vec_container in [statement.prefix.iter_mut(), statement.suffix.iter_mut()] {
            for container in vec_container {
                self.visit_glyph_container(container);
            }
        }
    }
    fn visit_ligature_subst(&self, statement: &mut fea_rs_ast::LigatureSubstStatement) {
        self.visit_glyph_container(&mut statement.replacement);
        for vec_container in [
            statement.glyphs.iter_mut(),
            statement.prefix.iter_mut(),
            statement.suffix.iter_mut(),
        ] {
            for container in vec_container {
                self.visit_glyph_container(container);
            }
        }
    }
    fn visit_reverse_chain_single_subst(
        &self,
        statement: &mut fea_rs_ast::ReverseChainSingleSubstStatement,
    ) {
        for vec_container in [
            statement.glyphs.iter_mut(),
            statement.replacements.iter_mut(),
            statement.prefix.iter_mut(),
            statement.suffix.iter_mut(),
        ] {
            for container in vec_container {
                self.visit_glyph_container(container);
            }
        }
    }

    fn visit_single_pos(&self, statement: &mut fea_rs_ast::SinglePosStatement) {
        for vec_container in [statement.prefix.iter_mut(), statement.suffix.iter_mut()] {
            for container in vec_container {
                self.visit_glyph_container(container);
            }
        }
        for (container, _vr) in statement.pos.iter_mut() {
            self.visit_glyph_container(container);
        }
    }
    fn visit_pair_pos(&self, statement: &mut fea_rs_ast::PairPosStatement) {
        self.visit_glyph_container(&mut statement.glyphs_1);
        self.visit_glyph_container(&mut statement.glyphs_2);
    }
    fn visit_cursive_pos(&self, statement: &mut fea_rs_ast::CursivePosStatement) {
        self.visit_glyph_container(&mut statement.glyphclass);
    }
    fn visit_mark_base_pos(&self, statement: &mut fea_rs_ast::MarkBasePosStatement) {
        self.visit_glyph_container(&mut statement.base);
    }
    fn visit_mark_lig_pos(&self, statement: &mut fea_rs_ast::MarkLigPosStatement) {
        self.visit_glyph_container(&mut statement.ligatures);
    }
    fn visit_mark_mark_pos(&self, statement: &mut fea_rs_ast::MarkMarkPosStatement) {
        self.visit_glyph_container(&mut statement.base_marks);
    }
    fn visit_chained_context<T: SubOrPos>(
        &self,
        statement: &mut fea_rs_ast::ChainedContextStatement<T>,
    ) {
        for vec_container in [
            statement.prefix.iter_mut(),
            statement.suffix.iter_mut(),
            statement.glyphs.iter_mut(),
        ] {
            for container in vec_container {
                self.visit_glyph_container(container);
            }
        }
    }
    fn visit_ignore<T: SubOrPos>(&self, statement: &mut fea_rs_ast::IgnoreStatement<T>) {
        for context in statement.chain_contexts.iter_mut() {
            for vec_container in [
                context.0.iter_mut(),
                context.1.iter_mut(),
                context.2.iter_mut(),
            ] {
                for container in vec_container {
                    self.visit_glyph_container(container);
                }
            }
        }
    }

    fn visit_mark_class_definition(&self, statement: &mut fea_rs_ast::MarkClassDefinition) {
        self.visit_glyph_container(&mut statement.glyphs);
    }
    fn visit_glyph_class_definition(&mut self, statement: &mut fea_rs_ast::GlyphClassDefinition) {
        for container in statement.glyphs.glyphs.iter_mut() {
            self.visit_glyph_container(container);
        }
    }
    fn visit_gdef_class_definition(&mut self, statement: &mut fea_rs_ast::GlyphClassDefStatement) {
        for x in statement.base_glyphs.iter_mut() {
            self.visit_glyph_container(x);
        }
        for container in statement.mark_glyphs.iter_mut() {
            self.visit_glyph_container(container);
        }
        for container in statement.ligature_glyphs.iter_mut() {
            self.visit_glyph_container(container);
        }
        for container in statement.component_glyphs.iter_mut() {
            self.visit_glyph_container(container);
        }
    }
    fn visit_gdef_attach(&mut self, statement: &mut fea_rs_ast::AttachStatement) {
        self.visit_glyph_container(&mut statement.glyphs);
    }
    fn visit_gdef_ligature_caret_by_index(
        &mut self,
        statement: &mut fea_rs_ast::LigatureCaretByIndexStatement,
    ) {
        self.visit_glyph_container(&mut statement.glyphs);
    }
    fn visit_gdef_ligature_caret_by_pos(
        &mut self,
        statement: &mut fea_rs_ast::LigatureCaretByPosStatement,
    ) {
        self.visit_glyph_container(&mut statement.glyphs);
    }

    fn visit_lookupflag(&mut self, lookupflag: &mut fea_rs_ast::LookupFlagStatement) {
        if let Some(ma) = lookupflag.mark_attachment.as_mut() {
            self.visit_glyph_container(ma);
        }
        // Same trick for mark filtering
        if let Some(ma) = lookupflag.mark_filtering_set.as_mut() {
            self.visit_glyph_container(ma);
        }
    }
}
impl LayoutVisitor for GlyphRenamerVisitor {
    fn depth_first(&self) -> bool {
        true
    }
    fn visit_statement(&mut self, statement: &mut Statement) -> bool {
        match statement {
            Statement::SingleSubst(single_subst_statement) => {
                self.visit_single_subst(single_subst_statement)
            }
            Statement::MultipleSubst(multiple_subst_statement) => {
                self.visit_multiple_subst(multiple_subst_statement)
            }
            Statement::AlternateSubst(alternate_subst_statement) => {
                self.visit_alternate_subst(alternate_subst_statement)
            }
            Statement::LigatureSubst(ligature_subst_statement) => {
                self.visit_ligature_subst(ligature_subst_statement)
            }
            Statement::ReverseChainSubst(reverse_chain_single_subst_statement) => {
                self.visit_reverse_chain_single_subst(reverse_chain_single_subst_statement)
            }
            Statement::ChainedContextSubst(chained_context_statement) => {
                self.visit_chained_context(chained_context_statement)
            }
            Statement::IgnoreSubst(ignore_statement) => self.visit_ignore(ignore_statement),
            Statement::SinglePos(single_pos_statement) => {
                self.visit_single_pos(single_pos_statement)
            }
            Statement::PairPos(pair_pos_statement) => self.visit_pair_pos(pair_pos_statement),
            Statement::CursivePos(cursive_pos_statement) => {
                self.visit_cursive_pos(cursive_pos_statement)
            }
            Statement::MarkBasePos(mark_base_pos_statement) => {
                self.visit_mark_base_pos(mark_base_pos_statement)
            }
            Statement::MarkLigPos(mark_lig_pos_statement) => {
                self.visit_mark_lig_pos(mark_lig_pos_statement)
            }
            Statement::MarkMarkPos(mark_mark_pos_statement) => {
                self.visit_mark_mark_pos(mark_mark_pos_statement)
            }
            Statement::ChainedContextPos(chained_context_statement) => {
                self.visit_chained_context(chained_context_statement)
            }
            Statement::IgnorePos(ignore_statement) => self.visit_ignore(ignore_statement),
            Statement::AnchorDefinition(_) => {}
            Statement::GdefAttach(attach_statement) => self.visit_gdef_attach(attach_statement),
            Statement::GdefClassDef(glyph_class_def_statement) => {
                self.visit_gdef_class_definition(glyph_class_def_statement)
            }
            Statement::GdefLigatureCaretByIndex(ligature_caret_by_index_statement) => {
                self.visit_gdef_ligature_caret_by_index(ligature_caret_by_index_statement)
            }
            Statement::GdefLigatureCaretByPos(ligature_caret_by_pos_statement) => {
                self.visit_gdef_ligature_caret_by_pos(ligature_caret_by_pos_statement)
            }
            Statement::MarkClassDefinition(mark_class_definition) => {
                self.visit_mark_class_definition(mark_class_definition)
            }
            Statement::Comment(_)
            | Statement::FeatureNameStatement(_)
            | Statement::FontRevision(_)
            | Statement::LookupReference(_)
            | Statement::FeatureReference(_)
            | Statement::Language(_)
            | Statement::LanguageSystem(_) => {}
            Statement::GlyphClassDefinition(glyph_class_definition) => {
                self.visit_glyph_class_definition(glyph_class_definition)
            }
            Statement::LookupFlag(lookupflag) => self.visit_lookupflag(lookupflag),
            Statement::SizeParameters(_)
            | Statement::SizeMenuName(_)
            | Statement::Subtable(_)
            | Statement::Script(_)
            | Statement::Gdef(_)
            | Statement::Head(_)
            | Statement::Hhea(_)
            | Statement::Name(_)
            | Statement::Stat(_)
            | Statement::Vhea(_)
            | Statement::Os2(_)
            | Statement::Base(_)
            | Statement::FeatureBlock(_)
            | Statement::LookupBlock(_)
            | Statement::NestedBlock(_)
            | Statement::ValueRecordDefinition(_)
            | Statement::ConditionSet(_)
            | Statement::VariationBlock(_) => {}
        }
        true
    }
}
