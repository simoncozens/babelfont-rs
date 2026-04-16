use crate::UncompileContext;
use fea_rs_ast::{
    AlternateSubstStatement, GlyphClass, GlyphContainer, LigatureSubstStatement, LookupBlock,
    MultipleSubstStatement, SingleSubstStatement, Statement, Subst,
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
        let gsub_lookup_list: LookupList<SubstitutionLookup> = match self.gsub.as_ref() {
            Some(gsub) => gsub.lookup_list()?,
            None => return Ok(()),
        };
        for (i, lookup) in gsub_lookup_list.lookups().iter().flatten().enumerate() {
            let subtables = lookup.subtables()?;
            match subtables {
                SubstitutionSubtables::Single(subtables) => {
                    let mut lookupblock =
                        self.create_next_lookup_block("gsub_single", i as u16, Subst);
                    for subtable in subtables.iter().flatten() {
                        match subtable {
                            SingleSubst::Format1(table_ref) => {
                                self.uncompile_gsub1_format1(&mut lookupblock, table_ref)?;
                            }
                            SingleSubst::Format2(table_ref) => {
                                self.uncompile_gsub1_format2(&mut lookupblock, table_ref)?;
                            }
                        }
                    }
                    self.lookups.insert(lookupblock.name.clone(), lookupblock);
                }
                SubstitutionSubtables::Multiple(subtables) => {
                    let mut lookupblock =
                        self.create_next_lookup_block("gsub_multiple", i as u16, Subst);
                    for subtable in subtables.iter().flatten() {
                        self.uncompile_gsub2(&mut lookupblock, subtable)?;
                    }
                    self.lookups.insert(lookupblock.name.clone(), lookupblock);
                }
                SubstitutionSubtables::Alternate(subtables) => {
                    let mut lookupblock =
                        self.create_next_lookup_block("gsub_alternate", i as u16, Subst);
                    for subtable in subtables.iter().flatten() {
                        self.uncompile_gsub3(&mut lookupblock, subtable)?;
                    }
                    self.lookups.insert(lookupblock.name.clone(), lookupblock);
                }
                SubstitutionSubtables::Ligature(subtables) => {
                    let mut lookupblock =
                        self.create_next_lookup_block("gsub_ligature", i as u16, Subst);
                    for subtable in subtables.iter().flatten() {
                        self.uncompile_gsub4(&mut lookupblock, subtable)?;
                    }
                    self.lookups.insert(lookupblock.name.clone(), lookupblock);
                }
                SubstitutionSubtables::Contextual(subtables) => {
                    let mut lookupblock =
                        self.create_next_lookup_block("gsub_contextual", i as u16, Subst);
                    for subtable in subtables.iter().flatten() {
                        lookupblock.statements.extend(
                            self.uncompile_sequence_context(subtable, Subst)?
                                .into_iter()
                                .map(Statement::ChainedContextSubst),
                        );
                    }
                    self.lookups.insert(lookupblock.name.clone(), lookupblock);
                }
                SubstitutionSubtables::ChainContextual(subtables) => {
                    let mut lookupblock =
                        self.create_next_lookup_block("gsub_chain_contextual", i as u16, Subst);
                    for subtable in subtables.iter().flatten() {
                        lookupblock.statements.extend(
                            self.uncompile_chain_sequence_context(subtable, Subst)?
                                .into_iter()
                                .map(Statement::ChainedContextSubst),
                        );
                    }
                    self.lookups.insert(lookupblock.name.clone(), lookupblock);
                }
                SubstitutionSubtables::Reverse(subtables) => {
                    let mut lookupblock =
                        self.create_next_lookup_block("gsub_reverse", i as u16, Subst);
                    for subtable in subtables.iter().flatten() {
                        self.uncompile_gsub7(&mut lookupblock, subtable)?;
                    }
                    self.lookups.insert(lookupblock.name.clone(), lookupblock);
                }
            }
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
            .map(|g| GlyphId16::new(g.to_u16().saturating_add_signed(delta)))
            .map(|g| GlyphContainer::GlyphName(self.get_name(g)))
            .collect::<Vec<GlyphContainer>>();
        let subst = SingleSubstStatement::new(inputs, replacements, vec![], vec![], 0..0, false);
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
        let subst = SingleSubstStatement::new(inputs, replacements, vec![], vec![], 0..0, false);
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
            let replacements = sequence
                .substitute_glyph_ids()
                .iter()
                .map(|g| GlyphContainer::GlyphName(self.get_name(g.get())))
                .collect();
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
        &self,
        _lookupblock: &mut LookupBlock,
        _gsub7: ReverseChainSingleSubstFormat1,
    ) -> Result<(), ReadError> {
        todo!()
    }
}
