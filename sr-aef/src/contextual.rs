use crate::UncompileContext;
use fea_rs_ast::{ChainedContextStatement, GlyphContainer, SubOrPos};
use skrifa::raw::{
    ReadError,
    tables::{gsub::ChainedSequenceContext, layout::SequenceContext},
};
use std::collections::HashMap;

impl<'a> UncompileContext<'a> {
    fn class_members_to_container(&self, members: Vec<GlyphContainer>) -> GlyphContainer {
        if members.len() == 1 {
            members.into_iter().next().unwrap()
        } else {
            GlyphContainer::GlyphClass(fea_rs_ast::GlyphClass::new(members, 0..0))
        }
    }

    pub(crate) fn uncompile_sequence_context<T: SubOrPos>(
        &mut self,
        sequence: SequenceContext,
        sub_or_pos: T,
    ) -> Result<Vec<ChainedContextStatement<T>>, ReadError> {
        let mut statements = vec![];
        match sequence {
            SequenceContext::Format1(seq_f1) => {
                let first_glyphs = seq_f1
                    .coverage()?
                    .iter()
                    .map(|g| GlyphContainer::GlyphName(self.get_name(g)))
                    .collect::<Vec<GlyphContainer>>();
                for (first_glyph, ruleset) in first_glyphs.iter().zip(seq_f1.seq_rule_sets().iter())
                {
                    if let Some(ruleset) = ruleset.transpose()? {
                        for rule in ruleset.seq_rules().iter().flatten() {
                            let mut input: Vec<_> = rule
                                .input_sequence()
                                .iter()
                                .map(|g| GlyphContainer::GlyphName(self.get_name(g.get())))
                                .collect();
                            input.insert(0, first_glyph.clone());
                            let mut lookups = vec![vec![]; input.len()];
                            for lookup_record in rule.seq_lookup_records() {
                                if let Some(lu) =
                                    lookups.get_mut(lookup_record.sequence_index() as usize)
                                {
                                    lu.push(self.get_lookup_name(
                                        lookup_record.lookup_list_index(),
                                        sub_or_pos,
                                    ));
                                }
                            }
                            let statement = ChainedContextStatement::new(
                                input,
                                vec![],
                                vec![],
                                lookups,
                                0..0,
                                sub_or_pos,
                            );
                            statements.push(statement);
                        }
                    }
                }
            }
            SequenceContext::Format2(_table_ref) => {
                let table_ref = _table_ref;
                let class_def = table_ref.class_def()?;
                let classes = self.resolve_classes(&class_def);

                let mut covered_input_classes: HashMap<u16, Vec<GlyphContainer>> = HashMap::new();
                for gid in table_ref.coverage()?.iter() {
                    let class_id = class_def.get(gid);
                    covered_input_classes
                        .entry(class_id)
                        .or_default()
                        .push(GlyphContainer::GlyphName(self.get_name(gid)));
                }

                for (class_id, ruleset) in table_ref.class_seq_rule_sets().iter().enumerate() {
                    let Some(ruleset) = ruleset.transpose()? else {
                        continue;
                    };
                    let Some(first_class_members) = covered_input_classes.get(&(class_id as u16))
                    else {
                        continue;
                    };
                    let first = self.class_members_to_container(first_class_members.clone());

                    for rule in ruleset.class_seq_rules().iter().flatten() {
                        let mut input = vec![first.clone()];
                        input.extend(rule.input_sequence().iter().map(|class_id| {
                            self.class_members_to_container(
                                classes.get(&class_id.get()).cloned().unwrap_or_default(),
                            )
                        }));

                        let mut lookups = vec![vec![]; input.len()];
                        for lookup_record in rule.seq_lookup_records() {
                            if let Some(lu) =
                                lookups.get_mut(lookup_record.sequence_index() as usize)
                            {
                                lu.push(self.get_lookup_name(
                                    lookup_record.lookup_list_index(),
                                    sub_or_pos,
                                ));
                            }
                        }

                        let statement = ChainedContextStatement::new(
                            input,
                            vec![],
                            vec![],
                            lookups,
                            0..0,
                            sub_or_pos,
                        );
                        statements.push(statement);
                    }
                }
            }
            SequenceContext::Format3(table_ref) => {
                let input: Vec<GlyphContainer> = table_ref
                    .coverages()
                    .iter()
                    .flatten()
                    .map(|coverage| self.resolve_coverage_to_class(&coverage))
                    .collect();
                let mut lookups = vec![vec![]; input.len()];
                for lookup_record in table_ref.seq_lookup_records() {
                    if let Some(lu) = lookups.get_mut(lookup_record.sequence_index() as usize) {
                        lu.push(
                            self.get_lookup_name(lookup_record.lookup_list_index(), sub_or_pos),
                        );
                    }
                }
                let statement =
                    ChainedContextStatement::new(input, vec![], vec![], lookups, 0..0, sub_or_pos);
                statements.push(statement);
            }
        }
        Ok(statements)
    }
    pub fn uncompile_chain_sequence_context<T: SubOrPos>(
        &mut self,
        chainsequence: ChainedSequenceContext,
        sub_or_pos: T,
    ) -> Result<Vec<ChainedContextStatement<T>>, ReadError> {
        let mut statements = vec![];
        match chainsequence {
            ChainedSequenceContext::Format1(seq_f1) => {
                let first_glyphs = seq_f1
                    .coverage()?
                    .iter()
                    .map(|g| GlyphContainer::GlyphName(self.get_name(g)))
                    .collect::<Vec<GlyphContainer>>();
                for (first_glyph, ruleset) in first_glyphs
                    .iter()
                    .zip(seq_f1.chained_seq_rule_sets().iter())
                {
                    if let Some(ruleset) = ruleset.transpose()? {
                        for rule in ruleset.chained_seq_rules().iter().flatten() {
                            let mut input: Vec<_> = rule
                                .input_sequence()
                                .iter()
                                .map(|g| GlyphContainer::GlyphName(self.get_name(g.get())))
                                .collect();
                            input.insert(0, first_glyph.clone());
                            let prefix = rule
                                .backtrack_sequence()
                                .iter()
                                .rev()
                                .map(|g| GlyphContainer::GlyphName(self.get_name(g.get())))
                                .collect::<Vec<GlyphContainer>>();
                            let suffix = rule
                                .lookahead_sequence()
                                .iter()
                                .map(|g| GlyphContainer::GlyphName(self.get_name(g.get())))
                                .collect::<Vec<GlyphContainer>>();
                            let mut lookups = vec![vec![]; input.len()];
                            for lookup_record in rule.seq_lookup_records() {
                                if let Some(lu) =
                                    lookups.get_mut(lookup_record.sequence_index() as usize)
                                {
                                    lu.push(self.get_lookup_name(
                                        lookup_record.lookup_list_index(),
                                        sub_or_pos,
                                    ));
                                }
                            }
                            let statement = ChainedContextStatement::new(
                                input,
                                prefix,
                                suffix,
                                lookups,
                                0..0,
                                sub_or_pos,
                            );
                            statements.push(statement);
                        }
                    }
                }
            }
            ChainedSequenceContext::Format2(_table_ref) => {
                let table_ref = _table_ref;
                let input_class_def = table_ref.input_class_def()?;
                let backtrack_class_def = table_ref.backtrack_class_def()?;
                let lookahead_class_def = table_ref.lookahead_class_def()?;

                let input_classes = self.resolve_classes(&input_class_def);
                let backtrack_classes = self.resolve_classes(&backtrack_class_def);
                let lookahead_classes = self.resolve_classes(&lookahead_class_def);

                let mut covered_input_classes: HashMap<u16, Vec<GlyphContainer>> = HashMap::new();
                for gid in table_ref.coverage()?.iter() {
                    let class_id = input_class_def.get(gid);
                    covered_input_classes
                        .entry(class_id)
                        .or_default()
                        .push(GlyphContainer::GlyphName(self.get_name(gid)));
                }

                for (class_id, ruleset) in
                    table_ref.chained_class_seq_rule_sets().iter().enumerate()
                {
                    let Some(ruleset) = ruleset.transpose()? else {
                        continue;
                    };
                    let Some(first_class_members) = covered_input_classes.get(&(class_id as u16))
                    else {
                        continue;
                    };
                    let first = self.class_members_to_container(first_class_members.clone());

                    for rule in ruleset.chained_class_seq_rules().iter().flatten() {
                        let mut input = vec![first.clone()];
                        input.extend(rule.input_sequence().iter().map(|class_id| {
                            self.class_members_to_container(
                                input_classes
                                    .get(&class_id.get())
                                    .cloned()
                                    .unwrap_or_default(),
                            )
                        }));

                        let mut prefix: Vec<GlyphContainer> = rule
                            .backtrack_sequence()
                            .iter()
                            .map(|class_id| {
                                self.class_members_to_container(
                                    backtrack_classes
                                        .get(&class_id.get())
                                        .cloned()
                                        .unwrap_or_default(),
                                )
                            })
                            .collect();
                        prefix.reverse();

                        let suffix: Vec<GlyphContainer> = rule
                            .lookahead_sequence()
                            .iter()
                            .map(|class_id| {
                                self.class_members_to_container(
                                    lookahead_classes
                                        .get(&class_id.get())
                                        .cloned()
                                        .unwrap_or_default(),
                                )
                            })
                            .collect();

                        let mut lookups = vec![vec![]; input.len()];
                        for lookup_record in rule.seq_lookup_records() {
                            if let Some(lu) =
                                lookups.get_mut(lookup_record.sequence_index() as usize)
                            {
                                lu.push(self.get_lookup_name(
                                    lookup_record.lookup_list_index(),
                                    sub_or_pos,
                                ));
                            }
                        }

                        let statement = ChainedContextStatement::new(
                            input,
                            prefix,
                            suffix,
                            lookups,
                            0..0,
                            sub_or_pos,
                        );
                        statements.push(statement);
                    }
                }
            }
            ChainedSequenceContext::Format3(table_ref) => {
                let input: Vec<GlyphContainer> = table_ref
                    .input_coverages()
                    .iter()
                    .flatten()
                    .map(|coverage| self.resolve_coverage_to_class(&coverage))
                    .collect();
                let mut pre: Vec<GlyphContainer> = table_ref
                    .backtrack_coverages()
                    .iter()
                    .flatten()
                    .map(|coverage| self.resolve_coverage_to_class(&coverage))
                    .collect();
                pre.reverse();
                let post: Vec<GlyphContainer> = table_ref
                    .lookahead_coverages()
                    .iter()
                    .flatten()
                    .map(|coverage| self.resolve_coverage_to_class(&coverage))
                    .collect();

                let mut lookups = vec![vec![]; input.len()];
                for lookup_record in table_ref.seq_lookup_records() {
                    if let Some(lu) = lookups.get_mut(lookup_record.sequence_index() as usize) {
                        lu.push(
                            self.get_lookup_name(lookup_record.lookup_list_index(), sub_or_pos),
                        );
                    }
                }
                let statement =
                    ChainedContextStatement::new(input, pre, post, lookups, 0..0, sub_or_pos);
                statements.push(statement);
            }
        }
        Ok(statements)
    }
}
