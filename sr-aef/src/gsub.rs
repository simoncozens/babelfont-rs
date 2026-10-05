use crate::{UncompileContext, add_subtable_break};
use fea_rs_ast::{
    AlternateSubstStatement, ChainedContextStatement, GlyphClass, GlyphContainer, GlyphName,
    IgnoreStatement, LigatureSubstStatement, LookupBlock, MultipleSubstStatement,
    ReverseChainSingleSubstStatement, SingleSubstStatement, Statement, Subst,
};
use skrifa::{
    GlyphId16,
    raw::{
        ReadError,
        tables::gsub::{
            AlternateSubstFormat1, LigatureSubstFormat1, LookupList, MultipleSubstFormat1,
            ReverseChainSingleSubstFormat1, SingleSubst, SingleSubstFormat1, SingleSubstFormat2,
            SubstitutionLookup, SubstitutionSubtables,
        },
    },
};
impl<'a> UncompileContext<'a> {
    pub(crate) fn uncompile_gsub_lookups(&mut self) -> Result<(), ReadError> {
        let gsub_lookup_list: LookupList<SubstitutionLookup> =
            match self.gsub.as_ref().and_then(|gsub| gsub.lookup_list().ok()) {
                Some(lookup_list) => lookup_list,
                None => return Ok(()),
            };
        for (i, lookup) in gsub_lookup_list.lookups().iter().flatten().enumerate() {
            let prefix = match lookup.subtables()? {
                SubstitutionSubtables::Single(_) => "gsub_single",
                SubstitutionSubtables::Multiple(_) => "gsub_multiple",
                SubstitutionSubtables::Alternate(_) => "gsub_alternate",
                SubstitutionSubtables::Ligature(_) => "gsub_ligature",
                SubstitutionSubtables::Contextual(_) => "gsub_contextual",
                SubstitutionSubtables::ChainContextual(_) => "gsub_chain_contextual",
                SubstitutionSubtables::Reverse(_) => "gsub_reverse",
                SubstitutionSubtables::EmptyExtension => "gsub_extension",
            };
            self.assign_lookup_name(prefix, i as u16, Subst);
        }
        for (i, lookup) in gsub_lookup_list.lookups().iter().flatten().enumerate() {
            let subtables = lookup.subtables()?;
            let mut lookupblock = match subtables {
                SubstitutionSubtables::Single(subtables) => {
                    let mut lookupblock = self.create_lookup_block(i as u16, Subst);
                    for subtable in subtables.iter().flatten() {
                        add_subtable_break(&mut lookupblock);
                        match subtable {
                            SingleSubst::Format1(table_ref) => {
                                self.uncompile_gsub1_format1(&mut lookupblock, table_ref)?;
                            }
                            SingleSubst::Format2(table_ref) => {
                                self.uncompile_gsub1_format2(&mut lookupblock, table_ref)?;
                            }
                        }
                    }
                    lookupblock
                }
                SubstitutionSubtables::Multiple(subtables) => {
                    let mut lookupblock = self.create_lookup_block(i as u16, Subst);
                    for subtable in subtables.iter().flatten() {
                        add_subtable_break(&mut lookupblock);
                        self.uncompile_gsub2(&mut lookupblock, subtable)?;
                    }
                    lookupblock
                }
                SubstitutionSubtables::Alternate(subtables) => {
                    let mut lookupblock = self.create_lookup_block(i as u16, Subst);
                    for subtable in subtables.iter().flatten() {
                        add_subtable_break(&mut lookupblock);
                        self.uncompile_gsub3(&mut lookupblock, subtable)?;
                    }
                    lookupblock
                }
                SubstitutionSubtables::Ligature(subtables) => {
                    let mut lookupblock = self.create_lookup_block(i as u16, Subst);
                    for subtable in subtables.iter().flatten() {
                        add_subtable_break(&mut lookupblock);
                        self.uncompile_gsub4(&mut lookupblock, subtable)?;
                    }
                    lookupblock
                }
                SubstitutionSubtables::Contextual(subtables) => {
                    let mut lookupblock = self.create_lookup_block(i as u16, Subst);
                    for subtable in subtables.iter().flatten() {
                        add_subtable_break(&mut lookupblock);
                        lookupblock.statements.extend(
                            self.uncompile_sequence_context(subtable, Subst)?
                                .into_iter()
                                .map(to_context_statement),
                        );
                    }
                    lookupblock
                }
                SubstitutionSubtables::ChainContextual(subtables) => {
                    let mut lookupblock = self.create_lookup_block(i as u16, Subst);
                    for subtable in subtables.iter().flatten() {
                        add_subtable_break(&mut lookupblock);
                        lookupblock.statements.extend(
                            self.uncompile_chain_sequence_context(subtable, Subst)?
                                .into_iter()
                                .map(to_context_statement),
                        );
                    }
                    lookupblock
                }
                SubstitutionSubtables::Reverse(subtables) => {
                    let mut lookupblock = self.create_lookup_block(i as u16, Subst);
                    for subtable in subtables.iter().flatten() {
                        add_subtable_break(&mut lookupblock);
                        self.uncompile_gsub7(&mut lookupblock, subtable)?;
                    }
                    lookupblock
                }

                SubstitutionSubtables::EmptyExtension => self.create_lookup_block(i as u16, Subst),
            };
            if let Some(Statement::Subtable(_)) = lookupblock.statements.last() {
                lookupblock.statements.pop();
            }
            self.add_lookup_flags(
                &mut lookupblock,
                lookup.lookup_flag(),
                lookup.mark_filtering_set(),
            );
            self.lookups.insert(lookupblock.name.clone(), lookupblock);
        }
        Ok(())
    }

    fn uncompile_gsub1_format1(
        &self,
        lookupblock: &mut LookupBlock,
        gsub1: SingleSubstFormat1,
    ) -> Result<(), ReadError> {
        let delta = gsub1.delta_glyph_id();
        let inputs = self.resolve_coverage(&gsub1.coverage()?);
        let replacements = gsub1
            .coverage()?
            .iter()
            .map(|g| GlyphId16::new(g.to_u16().wrapping_add_signed(delta)))
            .map(|g| GlyphContainer::GlyphName(self.get_name(g)))
            .collect::<Vec<GlyphContainer>>();
        let subst = SingleSubstStatement::new(
            vec![self.class_members_to_container(inputs)],
            vec![self.class_members_to_container(replacements)],
            vec![],
            vec![],
            0..0,
            false,
        );
        lookupblock.statements.push(Statement::SingleSubst(subst));

        Ok(())
    }
    fn uncompile_gsub1_format2(
        &self,
        lookupblock: &mut LookupBlock,
        gsub1: SingleSubstFormat2,
    ) -> Result<(), ReadError> {
        let inputs = self.resolve_coverage(&gsub1.coverage()?);
        let replacements = gsub1
            .substitute_glyph_ids()
            .iter()
            .map(|g| GlyphContainer::GlyphName(self.get_name(g.get())))
            .collect();
        let subst = SingleSubstStatement::new(
            vec![self.class_members_to_container(inputs)],
            vec![self.class_members_to_container(replacements)],
            vec![],
            vec![],
            0..0,
            false,
        );
        lookupblock.statements.push(Statement::SingleSubst(subst));
        Ok(())
    }
    fn uncompile_gsub2(
        &self,
        lookupblock: &mut LookupBlock,
        gsub2: MultipleSubstFormat1,
    ) -> Result<(), ReadError> {
        let inputs = self.resolve_coverage(&gsub2.coverage()?);
        for (input, sequence) in inputs.iter().zip(gsub2.sequences().iter().flatten()) {
            let mut replacements: Vec<GlyphContainer> = sequence
                .substitute_glyph_ids()
                .iter()
                .map(|g| GlyphContainer::GlyphName(self.get_name(g.get())))
                .collect();
            if replacements.is_empty() {
                replacements.push(GlyphContainer::GlyphName(GlyphName::new("NULL")));
            }
            let subst = MultipleSubstStatement::new(
                input.clone(),
                replacements,
                vec![],
                vec![],
                0..0,
                false,
            );
            lookupblock.statements.push(Statement::MultipleSubst(subst));
        }
        Ok(())
    }
    fn uncompile_gsub3(
        &self,
        _lookupblock: &mut LookupBlock,
        gsub3: AlternateSubstFormat1,
    ) -> Result<(), ReadError> {
        let inputs = self.resolve_coverage(&gsub3.coverage()?);
        for (input, alternate_set) in inputs.iter().zip(gsub3.alternate_sets().iter().flatten()) {
            let alternates = alternate_set
                .alternate_glyph_ids()
                .iter()
                .map(|g| GlyphContainer::GlyphName(self.get_name(g.get())))
                .collect();
            let subst = AlternateSubstStatement::new(
                input.clone(),
                GlyphContainer::GlyphClass(GlyphClass::new(alternates, 0..0)),
                vec![],
                vec![],
                0..0,
                false,
            );
            _lookupblock
                .statements
                .push(Statement::AlternateSubst(subst));
        }
        {}
        Ok(())
    }
    fn uncompile_gsub4(
        &self,
        lookupblock: &mut LookupBlock,
        gsub4: LigatureSubstFormat1,
    ) -> Result<(), ReadError> {
        let inputs = self.resolve_coverage(&gsub4.coverage()?);
        for (input, ligature_set) in inputs.iter().zip(gsub4.ligature_sets().iter().flatten()) {
            for ligature in ligature_set.ligatures().iter().flatten() {
                let mut components: Vec<GlyphContainer> = ligature
                    .component_glyph_ids()
                    .iter()
                    .map(|g| GlyphContainer::GlyphName(self.get_name(g.get())))
                    .collect();
                components.insert(0, input.clone());
                let subst = LigatureSubstStatement::new(
                    components,
                    GlyphContainer::GlyphName(self.get_name(ligature.ligature_glyph())),
                    vec![],
                    vec![],
                    0..0,
                    false,
                );
                lookupblock.statements.push(Statement::LigatureSubst(subst));
            }
        }
        Ok(())
    }

    fn uncompile_gsub7(
        &mut self,
        lookupblock: &mut LookupBlock,
        gsub7: ReverseChainSingleSubstFormat1,
    ) -> Result<(), ReadError> {
        let inputs = self.resolve_coverage(&gsub7.coverage()?);
        let replacements = gsub7
            .substitute_glyph_ids()
            .iter()
            .map(|g| GlyphContainer::GlyphName(self.get_name(g.get())))
            .collect();
        let mut prefix: Vec<GlyphContainer> = gsub7
            .backtrack_coverages()
            .iter()
            .flatten()
            .map(|coverage| self.resolve_coverage_to_class(&coverage, true))
            .collect();
        prefix.reverse();
        let suffix = gsub7
            .lookahead_coverages()
            .iter()
            .flatten()
            .map(|coverage| self.resolve_coverage_to_class(&coverage, true))
            .collect();
        let subst = ReverseChainSingleSubstStatement::new(
            vec![self.class_members_to_container(inputs)],
            vec![self.class_members_to_container(replacements)],
            prefix,
            suffix,
            0..0,
        );
        lookupblock
            .statements
            .push(Statement::ReverseChainSubst(subst));
        Ok(())
    }
}

fn to_context_statement(statement: ChainedContextStatement<Subst>) -> Statement {
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
