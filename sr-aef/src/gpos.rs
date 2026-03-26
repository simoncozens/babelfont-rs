use crate::UncompileContext;
use fea_rs_ast::{LookupBlock, Pos, Statement};
use skrifa::{
    GlyphId16,
    raw::{
        ReadError,
        tables::{
            gpos::{
                CursivePosFormat1, MarkBasePosFormat1, MarkLigPosFormat1, MarkMarkPosFormat1,
                PairPos, PairPosFormat1, PairPosFormat2, PositionLookup, PositionSubtables,
                SinglePos, SinglePosFormat1, SinglePosFormat2,
            },
            gsub::LookupList,
        },
    },
};
impl<'a> UncompileContext<'a> {
    pub(crate) fn uncompile_pos_lookups(&mut self) -> Result<(), ReadError> {
        let gpos_lookup_list: LookupList<PositionLookup> = match self.gpos.as_ref() {
            Some(gpos) => gpos.lookup_list()?,
            None => return Ok(()),
        };
        for lookup in gpos_lookup_list.lookups().iter().flatten() {
            let subtables = lookup.subtables()?;
            match subtables {
                PositionSubtables::Single(subtables) => {
                    let mut lookupblock = self.create_next_lookup_block("gsub_single");
                    for subtable in subtables.iter().flatten() {
                        match subtable {
                            SinglePos::Format1(gpos1f1) => {
                                self.uncompile_gpos1_format1(&mut lookupblock, gpos1f1)?;
                            }
                            SinglePos::Format2(gpos1f2) => {
                                self.uncompile_gpos1_format2(&mut lookupblock, gpos1f2)?;
                            }
                        }
                    }
                    self.lookups.insert(lookupblock.name.clone(), lookupblock);
                }
                PositionSubtables::Pair(subtables) => {
                    let mut lookupblock = self.create_next_lookup_block("gpos_pair");
                    for subtable in subtables.iter().flatten() {
                        match subtable {
                            PairPos::Format1(table_ref) => {
                                self.uncompile_gpos2_format1(&mut lookupblock, table_ref)?;
                            }
                            PairPos::Format2(table_ref) => {
                                self.uncompile_gpos2_format2(&mut lookupblock, table_ref)?;
                            }
                        }
                    }
                    self.lookups.insert(lookupblock.name.clone(), lookupblock);
                }
                PositionSubtables::Cursive(subtables) => {
                    let mut lookupblock = self.create_next_lookup_block("gpos_cursive");
                    for subtable in subtables.iter().flatten() {
                        self.uncompile_gpos3(&mut lookupblock, subtable)?;
                    }
                    self.lookups.insert(lookupblock.name.clone(), lookupblock);
                }
                PositionSubtables::MarkToBase(subtables) => {
                    let mut lookupblock = self.create_next_lookup_block("gpos_mark_to_base");
                    for subtable in subtables.iter().flatten() {
                        self.uncompile_gpos4(&mut lookupblock, subtable)?;
                    }
                    self.lookups.insert(lookupblock.name.clone(), lookupblock);
                }
                PositionSubtables::MarkToLig(subtables) => {
                    let mut lookupblock = self.create_next_lookup_block("gpos_mark_to_ligature");
                    for subtable in subtables.iter().flatten() {
                        self.uncompile_gpos5(&mut lookupblock, subtable)?;
                    }
                    self.lookups.insert(lookupblock.name.clone(), lookupblock);
                }
                PositionSubtables::MarkToMark(subtables) => {
                    let mut lookupblock = self.create_next_lookup_block("gpos_mark_to_mark");
                    for subtable in subtables.iter().flatten() {
                        self.uncompile_gpos6(&mut lookupblock, subtable)?;
                    }
                    self.lookups.insert(lookupblock.name.clone(), lookupblock);
                }
                PositionSubtables::Contextual(subtables) => {
                    let mut lookupblock = self.create_next_lookup_block("gpos_contextual");
                    for subtable in subtables.iter().flatten() {
                        lookupblock.statements.extend(
                            self.uncompile_sequence_context(subtable, Pos)?
                                .into_iter()
                                .map(Statement::ChainedContextPos),
                        );
                    }
                    self.lookups.insert(lookupblock.name.clone(), lookupblock);
                }
                PositionSubtables::ChainContextual(subtables) => {
                    let mut lookupblock = self.create_next_lookup_block("gpos_chain_contextual");
                    for subtable in subtables.iter().flatten() {
                        lookupblock.statements.extend(
                            self.uncompile_chain_sequence_context(subtable, Pos)?
                                .into_iter()
                                .map(Statement::ChainedContextPos),
                        );
                    }
                    self.lookups.insert(lookupblock.name.clone(), lookupblock);
                }
            }
        }
        Ok(())
    }

    fn uncompile_gpos1_format1(
        &self,
        lookupblock: &mut LookupBlock,
        gpos1f1: SinglePosFormat1,
    ) -> Result<(), ReadError> {
        todo!()
    }
    fn uncompile_gpos1_format2(
        &self,
        lookupblock: &mut LookupBlock,
        gpos1f2: SinglePosFormat2,
    ) -> Result<(), ReadError> {
        todo!()
    }

    fn uncompile_gpos2_format1(
        &self,
        lookupblock: &mut LookupBlock,
        gpos2f1: PairPosFormat1,
    ) -> Result<(), ReadError> {
        todo!()
    }
    fn uncompile_gpos2_format2(
        &self,
        lookupblock: &mut LookupBlock,
        gpos2f2: PairPosFormat2,
    ) -> Result<(), ReadError> {
        todo!()
    }
    fn uncompile_gpos3(
        &self,
        lookupblock: &mut LookupBlock,
        gpos3: CursivePosFormat1,
    ) -> Result<(), ReadError> {
        todo!()
    }
    fn uncompile_gpos4(
        &self,
        lookupblock: &mut LookupBlock,
        gpos4: MarkBasePosFormat1,
    ) -> Result<(), ReadError> {
        todo!()
    }
    fn uncompile_gpos5(
        &self,
        lookupblock: &mut LookupBlock,
        gpos5: MarkLigPosFormat1,
    ) -> Result<(), ReadError> {
        todo!()
    }
    fn uncompile_gpos6(
        &self,
        lookupblock: &mut LookupBlock,
        gpos6: MarkMarkPosFormat1,
    ) -> Result<(), ReadError> {
        todo!()
    }
}
