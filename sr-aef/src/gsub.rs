//! Un-compiling of GSUB lookups.

use crate::UncompileContext;
use crate::contextual::to_context_statement;
use crate::layout::{
    AlternateSubstSource, LigatureSubstSource, LookupKind, MultipleSubstSource,
    ReverseChainSubstSource, SingleSubstSource, UncompileSubtables, impl_uncompile_subtables,
};
use fea_rs_ast::AsFea;
use fea_rs_ast::{
    AlternateSubstStatement, GlyphClass, GlyphContainer, GlyphName, LigatureSubstStatement,
    LookupBlock, MultipleSubstStatement, ReverseChainSingleSubstStatement, SingleSubstStatement,
    Statement, Subst,
};
use skrifa::raw::{
    ReadError,
    tables::gsub::{
        AlternateSubst, ExtensionSubstFormat1, LigatureSubst, MultipleSubst,
        ReverseChainSingleSubst, SingleSubst, SubstitutionChainContext, SubstitutionLookup,
        SubstitutionSequenceContext, SubstitutionSubtables,
    },
};

impl<'a> UncompileContext<'a> {
    pub(crate) fn uncompile_gsub_lookups(&mut self) -> Result<(), ReadError> {
        let Some(lookup_list) = self.gsub.as_ref().and_then(|gsub| gsub.lookup_list().ok()) else {
            return Ok(());
        };
        // Name every lookup first: contextual lookups refer to lookups which
        // may come later in the list.
        for (i, lookup) in lookup_list.lookups().iter().flatten().enumerate() {
            self.assign_lookup_name(lookup.subtables()?.prefix(), i as u16, Subst);
        }
        for (i, lookup) in lookup_list.lookups().iter().flatten().enumerate() {
            let mut block = lookup.subtables()?.uncompile(self, i as u16, Subst)?;
            if let Some(Statement::Subtable(_)) = block.statements.last() {
                block.statements.pop();
            }
            block.use_extension = matches!(lookup, SubstitutionLookup::Extension(_));
            self.add_lookup_flags(
                &mut block,
                lookup.lookup_flag(),
                lookup.mark_filtering_set(),
            );
            self.lookups.insert(block.name.clone(), block);
        }
        Ok(())
    }

    fn uncompile_single_subst(
        &mut self,
        block: &mut LookupBlock,
        subtable: &SingleSubst<'a>,
    ) -> Result<(), ReadError> {
        let coverage = subtable.coverage()?;
        let replacements = subtable.substitutes()?;
        let statement = SingleSubstStatement::new(
            vec![self.class_members_to_container(self.resolve_coverage(&coverage))],
            vec![
                self.class_members_to_container(
                    replacements
                        .into_iter()
                        .map(|glyph| GlyphContainer::GlyphName(self.get_name(glyph)))
                        .collect(),
                ),
            ],
            vec![],
            vec![],
            0..0,
            false,
        );
        block.statements.push(Statement::SingleSubst(statement));
        Ok(())
    }

    fn uncompile_multiple_subst(
        &mut self,
        block: &mut LookupBlock,
        subtable: &MultipleSubst<'a>,
    ) -> Result<(), ReadError> {
        let inputs = self.resolve_coverage(&subtable.coverage()?);
        for (input, sequence) in inputs.iter().zip(subtable.sequences()) {
            let mut replacements: Vec<GlyphContainer> = sequence
                .iter()
                .map(|glyph| GlyphContainer::GlyphName(self.get_name(*glyph)))
                .collect();
            if replacements.is_empty() {
                replacements.push(GlyphContainer::GlyphName(GlyphName::new("NULL")));
            }
            let statement = MultipleSubstStatement::new(
                input.clone(),
                replacements,
                vec![],
                vec![],
                0..0,
                false,
            );
            block.statements.push(Statement::MultipleSubst(statement));
        }
        Ok(())
    }

    fn uncompile_alternate_subst(
        &mut self,
        block: &mut LookupBlock,
        subtable: &AlternateSubst<'a>,
    ) -> Result<(), ReadError> {
        let inputs = self.resolve_coverage(&subtable.coverage()?);
        for (input, alternates) in inputs.iter().zip(subtable.alternate_sets()) {
            let alternates = alternates
                .iter()
                .map(|glyph| GlyphContainer::GlyphName(self.get_name(*glyph)))
                .collect();
            let statement = AlternateSubstStatement::new(
                input.clone(),
                GlyphContainer::GlyphClass(GlyphClass::new(alternates, 0..0)),
                vec![],
                vec![],
                0..0,
                false,
            );
            block.statements.push(Statement::AlternateSubst(statement));
        }
        Ok(())
    }

    fn uncompile_ligature_subst(
        &mut self,
        block: &mut LookupBlock,
        subtable: &LigatureSubst<'a>,
    ) -> Result<(), ReadError> {
        let inputs = self.resolve_coverage(&subtable.coverage()?);
        // a sequence an earlier rule of the lookup ligates never reaches a
        // later one (a feature file may not state both)
        let sequence = |components: &[GlyphContainer]| {
            components
                .iter()
                .map(|c| c.as_fea(""))
                .collect::<Vec<_>>()
                .join(" ")
        };
        let mut seen: std::collections::HashSet<String> = block
            .statements
            .iter()
            .filter_map(|s| match s {
                Statement::LigatureSubst(subst) => Some(sequence(&subst.glyphs)),
                _ => None,
            })
            .collect();
        for (input, ligatures) in inputs.iter().zip(subtable.ligature_sets()) {
            for ligature in ligatures {
                let mut components: Vec<GlyphContainer> = ligature
                    .components
                    .iter()
                    .map(|glyph| GlyphContainer::GlyphName(self.get_name(*glyph)))
                    .collect();
                components.insert(0, input.clone());
                if !seen.insert(sequence(&components)) {
                    block
                        .statements
                        .push(Statement::Comment(fea_rs_ast::Comment::new(format!(
                            "# sub {} by {}; is shadowed by an earlier rule",
                            sequence(&components),
                            self.get_name(ligature.glyph).as_fea("")
                        ))));
                    continue;
                }
                let statement = LigatureSubstStatement::new(
                    components,
                    GlyphContainer::GlyphName(self.get_name(ligature.glyph)),
                    vec![],
                    vec![],
                    0..0,
                    false,
                );
                block.statements.push(Statement::LigatureSubst(statement));
            }
        }
        Ok(())
    }

    fn uncompile_reverse_chain_subst(
        &mut self,
        block: &mut LookupBlock,
        subtable: &ReverseChainSingleSubst<'a>,
    ) -> Result<(), ReadError> {
        let coverage = subtable.coverage()?;
        let inputs = self.resolve_coverage(&coverage);
        let replacements = subtable
            .substitutes()
            .into_iter()
            .map(|glyph| GlyphContainer::GlyphName(self.get_name(glyph)))
            .collect();
        let mut prefix: Vec<GlyphContainer> = subtable
            .backtrack()
            .iter()
            .map(|coverage| self.resolve_coverage_to_class(coverage, true))
            .collect();
        prefix.reverse();
        let suffix = subtable
            .lookahead()
            .iter()
            .map(|coverage| self.resolve_coverage_to_class(coverage, true))
            .collect();
        let statement = ReverseChainSingleSubstStatement::new(
            vec![self.class_members_to_container(inputs)],
            vec![self.class_members_to_container(replacements)],
            prefix,
            suffix,
            0..0,
        );
        block
            .statements
            .push(Statement::ReverseChainSubst(statement));
        Ok(())
    }
}

/// GSUB lookup type 1.
struct SingleSubstKind;

impl<'a> LookupKind<'a> for SingleSubstKind {
    type Subtable = SingleSubst<'a>;
    type Extension = ExtensionSubstFormat1<'a, SingleSubst<'a>>;
    const PREFIX: &'static str = "gsub_single";

    fn uncompile_subtable(
        ctx: &mut UncompileContext<'a>,
        block: &mut LookupBlock,
        subtable: Self::Subtable,
    ) -> Result<(), ReadError> {
        ctx.uncompile_single_subst(block, &subtable)
    }
}

/// GSUB lookup type 2.
struct MultipleSubstKind;

impl<'a> LookupKind<'a> for MultipleSubstKind {
    type Subtable = MultipleSubst<'a>;
    type Extension = ExtensionSubstFormat1<'a, MultipleSubst<'a>>;
    const PREFIX: &'static str = "gsub_multiple";

    fn uncompile_subtable(
        ctx: &mut UncompileContext<'a>,
        block: &mut LookupBlock,
        subtable: Self::Subtable,
    ) -> Result<(), ReadError> {
        ctx.uncompile_multiple_subst(block, &subtable)
    }
}

/// GSUB lookup type 3.
struct AlternateSubstKind;

impl<'a> LookupKind<'a> for AlternateSubstKind {
    type Subtable = AlternateSubst<'a>;
    type Extension = ExtensionSubstFormat1<'a, AlternateSubst<'a>>;
    const PREFIX: &'static str = "gsub_alternate";

    fn uncompile_subtable(
        ctx: &mut UncompileContext<'a>,
        block: &mut LookupBlock,
        subtable: Self::Subtable,
    ) -> Result<(), ReadError> {
        ctx.uncompile_alternate_subst(block, &subtable)
    }
}

/// GSUB lookup type 4.
struct LigatureSubstKind;

impl<'a> LookupKind<'a> for LigatureSubstKind {
    type Subtable = LigatureSubst<'a>;
    type Extension = ExtensionSubstFormat1<'a, LigatureSubst<'a>>;
    const PREFIX: &'static str = "gsub_ligature";

    fn uncompile_subtable(
        ctx: &mut UncompileContext<'a>,
        block: &mut LookupBlock,
        subtable: Self::Subtable,
    ) -> Result<(), ReadError> {
        ctx.uncompile_ligature_subst(block, &subtable)
    }
}

/// GSUB lookup type 5.
struct ContextualKind;

impl<'a> LookupKind<'a> for ContextualKind {
    type Subtable = SubstitutionSequenceContext<'a>;
    type Extension = ExtensionSubstFormat1<'a, SubstitutionSequenceContext<'a>>;
    const PREFIX: &'static str = "gsub_contextual";

    fn uncompile_subtable(
        ctx: &mut UncompileContext<'a>,
        block: &mut LookupBlock,
        subtable: Self::Subtable,
    ) -> Result<(), ReadError> {
        block.statements.extend(
            ctx.uncompile_sequence_context(subtable, Subst)?
                .into_iter()
                .map(to_context_statement),
        );
        Ok(())
    }
}

/// GSUB lookup type 6.
struct ChainContextualKind;

impl<'a> LookupKind<'a> for ChainContextualKind {
    type Subtable = SubstitutionChainContext<'a>;
    type Extension = ExtensionSubstFormat1<'a, SubstitutionChainContext<'a>>;
    const PREFIX: &'static str = "gsub_chain_contextual";

    fn uncompile_subtable(
        ctx: &mut UncompileContext<'a>,
        block: &mut LookupBlock,
        subtable: Self::Subtable,
    ) -> Result<(), ReadError> {
        block.statements.extend(
            ctx.uncompile_chain_sequence_context(subtable, Subst)?
                .into_iter()
                .map(to_context_statement),
        );
        Ok(())
    }
}

/// GSUB lookup type 8.
struct ReverseChainSubstKind;

impl<'a> LookupKind<'a> for ReverseChainSubstKind {
    type Subtable = ReverseChainSingleSubst<'a>;
    type Extension = ExtensionSubstFormat1<'a, ReverseChainSingleSubst<'a>>;
    const PREFIX: &'static str = "gsub_reverse";

    fn uncompile_subtable(
        ctx: &mut UncompileContext<'a>,
        block: &mut LookupBlock,
        subtable: Self::Subtable,
    ) -> Result<(), ReadError> {
        ctx.uncompile_reverse_chain_subst(block, &subtable)
    }
}

impl_uncompile_subtables!(SubstitutionSubtables, empty = "gsub_extension", {
    Single => SingleSubstKind,
    Multiple => MultipleSubstKind,
    Alternate => AlternateSubstKind,
    Ligature => LigatureSubstKind,
    Contextual => ContextualKind,
    ChainContextual => ChainContextualKind,
    Reverse => ReverseChainSubstKind,
});
