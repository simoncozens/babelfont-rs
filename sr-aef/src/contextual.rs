//! Un-compiling of contextual lookups, shared by GSUB and GPOS.
//!
//! Context and chained-context subtables each come in six and five formats
//! respectively, but only express three different shapes: rules written in
//! terms of glyphs, rules written in terms of classes, and a single rule
//! written as a list of coverages. [`crate::layout`] folds the formats which
//! share a shape together, so this file only deals with the shapes.

use std::collections::HashMap;

use fea_rs_ast::{
    ChainedContextStatement, GlyphContainer, IgnoreStatement, Pos, Statement, SubOrPos, Subst,
};
use skrifa::{
    GlyphId,
    raw::{
        ReadError,
        tables::layout::{ChainedSequenceContext, CoverageTable, SequenceContext},
    },
};
use smol_str::SmolStr;

use crate::UncompileContext;
use crate::layout::{ChainedSequenceContextSource, SequenceContextData, SequenceContextSource};

/// The phase-dependent parts of turning a context rule into a statement.
pub(crate) trait ContextPhase: SubOrPos {
    fn into_statement(statement: ChainedContextStatement<Self>) -> Statement;
}

impl ContextPhase for Subst {
    fn into_statement(statement: ChainedContextStatement<Subst>) -> Statement {
        if statement.lookups.iter().all(Vec::is_empty) {
            Statement::IgnoreSubst(IgnoreStatement::new(
                vec![(statement.prefix, statement.glyphs, statement.suffix)],
                0..0,
                Subst,
            ))
        } else {
            Statement::ChainedContextSubst(statement)
        }
    }
}

impl ContextPhase for Pos {
    fn into_statement(statement: ChainedContextStatement<Pos>) -> Statement {
        if statement.lookups.iter().all(Vec::is_empty) {
            Statement::IgnorePos(IgnoreStatement::new(
                vec![(statement.prefix, statement.glyphs, statement.suffix)],
                0..0,
                Pos,
            ))
        } else {
            Statement::ChainedContextPos(statement)
        }
    }
}

pub(crate) fn to_context_statement<T: ContextPhase>(
    statement: ChainedContextStatement<T>,
) -> Statement {
    T::into_statement(statement)
}

impl<'a> UncompileContext<'a> {
    pub(crate) fn class_members_to_container(
        &self,
        members: Vec<GlyphContainer>,
    ) -> GlyphContainer {
        if members.len() == 1 {
            members.into_iter().next().unwrap()
        } else {
            GlyphContainer::GlyphClass(fea_rs_ast::GlyphClass::new(members, 0..0))
        }
    }

    pub(crate) fn glyph_containers(&self, glyphs: &[GlyphId]) -> Vec<GlyphContainer> {
        glyphs
            .iter()
            .map(|glyph| GlyphContainer::GlyphName(self.get_name(*glyph)))
            .collect()
    }

    fn class_containers(
        &self,
        classes: &[u32],
        class_members: &HashMap<u32, Vec<GlyphContainer>>,
    ) -> Vec<GlyphContainer> {
        classes
            .iter()
            .map(|class| {
                self.class_members_to_container(
                    class_members.get(class).cloned().unwrap_or_default(),
                )
            })
            .collect()
    }

    /// Group a rule's lookup references by the input position they apply to.
    fn lookup_slots<T: SubOrPos>(
        &self,
        references: &[(u16, u16)],
        input_len: usize,
        phase: T,
    ) -> Vec<Vec<SmolStr>> {
        let mut slots = vec![vec![]; input_len];
        for (sequence_index, lookup_index) in references {
            if let Some(slot) = slots.get_mut(*sequence_index as usize) {
                slot.push(self.get_lookup_name(*lookup_index, phase));
            }
        }
        slots
    }

    /// A statement for a rule whose input starts with `first`.
    fn context_statement<T: SubOrPos>(
        &self,
        first: GlyphContainer,
        input: Vec<GlyphContainer>,
        prefix: Vec<GlyphContainer>,
        suffix: Vec<GlyphContainer>,
        references: &[(u16, u16)],
        phase: T,
    ) -> ChainedContextStatement<T> {
        let mut glyphs = vec![first];
        glyphs.extend(input);
        let lookups = self.lookup_slots(references, glyphs.len(), phase);
        ChainedContextStatement::new(glyphs, prefix, suffix, lookups, 0..0, phase)
    }

    pub(crate) fn uncompile_sequence_context<T: SubOrPos>(
        &mut self,
        sequence: SequenceContext<'a>,
        phase: T,
    ) -> Result<Vec<ChainedContextStatement<T>>, ReadError> {
        let mut statements = vec![];
        match sequence.data()? {
            SequenceContextData::GlyphRules {
                coverage,
                rule_sets,
            } => {
                for (first_glyph, rules) in coverage.iter().zip(rule_sets) {
                    let first = GlyphContainer::GlyphName(self.get_name(*first_glyph));
                    for rule in rules {
                        let input = self.glyph_containers(&rule.input);
                        statements.push(self.context_statement(
                            first.clone(),
                            input,
                            vec![],
                            vec![],
                            &rule.lookups,
                            phase,
                        ));
                    }
                }
            }
            SequenceContextData::ClassRules {
                coverage,
                class_def,
                rule_sets,
            } => {
                let classes = self.resolve_classes(Some(&class_def));
                let mut covered_input_classes: HashMap<u32, Vec<GlyphContainer>> = HashMap::new();
                for glyph in &coverage {
                    covered_input_classes
                        .entry(class_def.get(*glyph))
                        .or_default()
                        .push(GlyphContainer::GlyphName(self.get_name(*glyph)));
                }
                for (class_index, rules) in rule_sets.iter().enumerate() {
                    let Some(members) = covered_input_classes.get(&(class_index as u32)) else {
                        continue;
                    };
                    let first = self.class_members_to_container(members.clone());
                    for rule in rules {
                        let input = self.class_containers(&rule.input, &classes);
                        statements.push(self.context_statement(
                            first.clone(),
                            input,
                            vec![],
                            vec![],
                            &rule.lookups,
                            phase,
                        ));
                    }
                }
            }
            SequenceContextData::Coverages { coverages, lookups } => {
                let input = self.coverage_containers(&coverages);
                let slots = self.lookup_slots(&lookups, input.len(), phase);
                statements.push(ChainedContextStatement::new(
                    input,
                    vec![],
                    vec![],
                    slots,
                    0..0,
                    phase,
                ));
            }
        }
        Ok(statements)
    }

    fn coverage_containers(&mut self, coverages: &[CoverageTable<'a>]) -> Vec<GlyphContainer> {
        coverages
            .iter()
            .map(|coverage| self.resolve_coverage_to_class(coverage, true))
            .collect()
    }

    pub(crate) fn uncompile_chain_sequence_context<T: SubOrPos>(
        &mut self,
        chainsequence: ChainedSequenceContext<'a>,
        phase: T,
    ) -> Result<Vec<ChainedContextStatement<T>>, ReadError> {
        let mut statements = vec![];
        match chainsequence.data()? {
            crate::layout::ChainedSequenceContextData::GlyphRules {
                coverage,
                rule_sets,
            } => {
                for (first_glyph, rules) in coverage.iter().zip(rule_sets) {
                    let first = GlyphContainer::GlyphName(self.get_name(*first_glyph));
                    for rule in rules {
                        let input = self.glyph_containers(&rule.input);
                        let prefix = self
                            .glyph_containers(&rule.backtrack)
                            .into_iter()
                            .rev()
                            .collect();
                        let suffix = self.glyph_containers(&rule.lookahead);
                        statements.push(self.context_statement(
                            first.clone(),
                            input,
                            prefix,
                            suffix,
                            &rule.lookups,
                            phase,
                        ));
                    }
                }
            }
            crate::layout::ChainedSequenceContextData::ClassRules {
                coverage,
                input_class_def,
                backtrack_class_def,
                lookahead_class_def,
                rule_sets,
            } => {
                let input_classes = self.resolve_classes(input_class_def.as_ref());
                let backtrack_classes = self.resolve_classes(backtrack_class_def.as_ref());
                let lookahead_classes = self.resolve_classes(lookahead_class_def.as_ref());

                let mut covered_input_classes: HashMap<u32, Vec<GlyphContainer>> = HashMap::new();
                for glyph in &coverage {
                    let class = input_class_def
                        .as_ref()
                        .map_or(0, |class_def| class_def.get(*glyph));
                    covered_input_classes
                        .entry(class)
                        .or_default()
                        .push(GlyphContainer::GlyphName(self.get_name(*glyph)));
                }

                for (class_index, rules) in rule_sets.iter().enumerate() {
                    let Some(members) = covered_input_classes.get(&(class_index as u32)) else {
                        continue;
                    };
                    let first = self.class_members_to_container(members.clone());
                    for rule in rules {
                        let input = self.class_containers(&rule.input, &input_classes);
                        let prefix = self
                            .class_containers(&rule.backtrack, &backtrack_classes)
                            .into_iter()
                            .rev()
                            .collect();
                        let suffix = self.class_containers(&rule.lookahead, &lookahead_classes);
                        statements.push(self.context_statement(
                            first.clone(),
                            input,
                            prefix,
                            suffix,
                            &rule.lookups,
                            phase,
                        ));
                    }
                }
            }
            crate::layout::ChainedSequenceContextData::Coverages {
                backtrack,
                input,
                lookahead,
                lookups,
            } => {
                let input = self.coverage_containers(&input);
                let prefix = self
                    .coverage_containers(&backtrack)
                    .into_iter()
                    .rev()
                    .collect();
                let suffix = self.coverage_containers(&lookahead);
                let slots = self.lookup_slots(&lookups, input.len(), phase);
                statements.push(ChainedContextStatement::new(
                    input,
                    prefix,
                    suffix,
                    slots,
                    0..0,
                    phase,
                ));
            }
        }
        Ok(statements)
    }
}
