use std::collections::HashMap;

use crate::{UncompileContext, add_subtable_break};
use fea_rs_ast::{
    Anchor as FeaAnchor, ChainedContextStatement, CursivePosStatement, GlyphClass, GlyphContainer,
    IgnoreStatement, LookupBlock, MarkBasePosStatement, MarkClass, MarkLigPosStatement,
    MarkMarkPosStatement, Metric, PairPosStatement, Pos, SinglePosStatement, Statement,
    ValueRecord as FeaValueRecord,
};
use indexmap::IndexMap;
use skrifa::raw::{
    FontData, ReadError,
    tables::{
        gpos::{
            AnchorTable, CursivePosFormat1, MarkBasePosFormat1, MarkLigPosFormat1,
            MarkMarkPosFormat1, PairPos, PairPosFormat1, PairPosFormat2, PositionLookup,
            PositionSubtables, SinglePos, SinglePosFormat1, SinglePosFormat2, ValueRecord,
        },
        gsub::LookupList,
    },
};
use smol_str::SmolStr;
impl<'a> UncompileContext<'a> {
    pub(crate) fn uncompile_gpos_lookups(&mut self) -> Result<(), ReadError> {
        let gpos_lookup_list: LookupList<PositionLookup> =
            match self.gpos.as_ref().and_then(|gpos| gpos.lookup_list().ok()) {
                Some(lookup_list) => lookup_list,
                None => return Ok(()),
            };
        for (i, lookup) in gpos_lookup_list.lookups().iter().flatten().enumerate() {
            let prefix = match lookup.subtables()? {
                PositionSubtables::Single(_) => "gpos_single",
                PositionSubtables::Pair(_) => "gpos_pair",
                PositionSubtables::Cursive(_) => "gpos_cursive",
                PositionSubtables::MarkToBase(_) => "gpos_mark_to_base",
                PositionSubtables::MarkToLig(_) => "gpos_mark_to_ligature",
                PositionSubtables::MarkToMark(_) => "gpos_mark_to_mark",
                PositionSubtables::Contextual(_) => "gpos_contextual",
                PositionSubtables::ChainContextual(_) => "gpos_chain_contextual",
                PositionSubtables::EmptyExtension => "gpos_extension",
            };
            self.assign_lookup_name(prefix, i as u16, Pos);
        }
        for (i, lookup) in gpos_lookup_list.lookups().iter().flatten().enumerate() {
            let subtables = lookup.subtables()?;
            let mut lookupblock = match subtables {
                PositionSubtables::Single(subtables) => {
                    let mut lookupblock = self.create_lookup_block(i as u16, Pos);
                    for subtable in subtables.iter().flatten() {
                        add_subtable_break(&mut lookupblock);
                        match subtable {
                            SinglePos::Format1(gpos1f1) => {
                                self.uncompile_gpos1_format1(&mut lookupblock, gpos1f1)?;
                            }
                            SinglePos::Format2(gpos1f2) => {
                                self.uncompile_gpos1_format2(&mut lookupblock, gpos1f2)?;
                            }
                        }
                    }
                    lookupblock
                }
                PositionSubtables::Pair(subtables) => {
                    let mut lookupblock = self.create_lookup_block(i as u16, Pos);
                    for subtable in subtables.iter().flatten() {
                        add_subtable_break(&mut lookupblock);
                        match subtable {
                            PairPos::Format1(table_ref) => {
                                self.uncompile_gpos2_format1(&mut lookupblock, table_ref)?;
                            }
                            PairPos::Format2(table_ref) => {
                                self.uncompile_gpos2_format2(&mut lookupblock, table_ref)?;
                            }
                        }
                    }
                    lookupblock
                }
                PositionSubtables::Cursive(subtables) => {
                    let mut lookupblock = self.create_lookup_block(i as u16, Pos);
                    for subtable in subtables.iter().flatten() {
                        add_subtable_break(&mut lookupblock);
                        self.uncompile_gpos3(&mut lookupblock, subtable)?;
                    }
                    lookupblock
                }
                PositionSubtables::MarkToBase(subtables) => {
                    let mut lookupblock = self.create_lookup_block(i as u16, Pos);
                    for subtable in subtables.iter().flatten() {
                        add_subtable_break(&mut lookupblock);
                        self.uncompile_gpos4(&mut lookupblock, subtable)?;
                    }
                    lookupblock
                }
                PositionSubtables::MarkToLig(subtables) => {
                    let mut lookupblock = self.create_lookup_block(i as u16, Pos);
                    for subtable in subtables.iter().flatten() {
                        add_subtable_break(&mut lookupblock);
                        self.uncompile_gpos5(&mut lookupblock, subtable)?;
                    }
                    lookupblock
                }
                PositionSubtables::MarkToMark(subtables) => {
                    let mut lookupblock = self.create_lookup_block(i as u16, Pos);
                    for subtable in subtables.iter().flatten() {
                        add_subtable_break(&mut lookupblock);
                        self.uncompile_gpos6(&mut lookupblock, subtable)?;
                    }
                    lookupblock
                }
                PositionSubtables::Contextual(subtables) => {
                    let mut lookupblock = self.create_lookup_block(i as u16, Pos);
                    for subtable in subtables.iter().flatten() {
                        add_subtable_break(&mut lookupblock);
                        lookupblock.statements.extend(
                            self.uncompile_sequence_context(subtable, Pos)?
                                .into_iter()
                                .map(to_context_statement),
                        );
                    }
                    lookupblock
                }
                PositionSubtables::ChainContextual(subtables) => {
                    let mut lookupblock = self.create_lookup_block(i as u16, Pos);
                    for subtable in subtables.iter().flatten() {
                        add_subtable_break(&mut lookupblock);
                        lookupblock.statements.extend(
                            self.uncompile_chain_sequence_context(subtable, Pos)?
                                .into_iter()
                                .map(to_context_statement),
                        );
                    }
                    lookupblock
                }
                PositionSubtables::EmptyExtension => self.create_lookup_block(i as u16, Pos),
            };
            if let Some(Statement::Subtable(_)) = lookupblock.statements.last() {
                lookupblock.statements.pop();
            }
            lookupblock.use_extension = matches!(lookup, PositionLookup::Extension(_));
            self.add_lookup_flags(
                &mut lookupblock,
                lookup.lookup_flag(),
                lookup.mark_filtering_set(),
            );
            self.lookups.insert(lookupblock.name.clone(), lookupblock);
        }
        Ok(())
    }

    fn resolve_value_record(
        &self,
        value_record: &ValueRecord,
        parent_offset_data: FontData<'_>,
    ) -> Result<FeaValueRecord, ReadError> {
        let x_placement = value_record
            .x_placement()
            .map(|vr| {
                self.resolve_pos_with_variations(
                    vr,
                    value_record.x_placement_device(parent_offset_data),
                )
            })
            .transpose()?;
        let y_placement = value_record
            .y_placement()
            .map(|vr| {
                self.resolve_pos_with_variations(
                    vr,
                    value_record.y_placement_device(parent_offset_data),
                )
            })
            .transpose()?;
        let x_advance = value_record
            .x_advance()
            .map(|vr| {
                self.resolve_pos_with_variations(
                    vr,
                    value_record.x_advance_device(parent_offset_data),
                )
            })
            .transpose()?;
        let y_advance = value_record
            .y_advance()
            .map(|vr| {
                self.resolve_pos_with_variations(
                    vr,
                    value_record.y_advance_device(parent_offset_data),
                )
            })
            .transpose()?;
        Ok(FeaValueRecord::new(
            x_placement,
            y_placement,
            x_advance,
            y_advance,
            None,
            None,
            None,
            None,
            false,
            0..0,
            None,
        ))
    }

    fn resolve_anchor(&self, anchor: &AnchorTable) -> Result<FeaAnchor, ReadError> {
        let x = self.resolve_pos_with_variations(anchor.x_coordinate(), anchor.x_device())?;
        let y = self.resolve_pos_with_variations(anchor.y_coordinate(), anchor.y_device())?;
        Ok(FeaAnchor::new(x, y, None, None, None, None, 0..0))
    }

    fn uncompile_gpos1_format1(
        &self,
        lookupblock: &mut LookupBlock,
        gpos1f1: SinglePosFormat1,
    ) -> Result<(), ReadError> {
        let input = self.resolve_coverage(&gpos1f1.coverage()?);
        let offset_data = gpos1f1.offset_data();
        let vr = self.resolve_value_record(&gpos1f1.value_record(), offset_data)?;
        let statement = SinglePosStatement::new(
            vec![],
            vec![],
            vec![(self.class_members_to_container(input), Some(vr))],
            false,
            0..0,
        );
        lookupblock.statements.push(Statement::SinglePos(statement));
        Ok(())
    }
    fn uncompile_gpos1_format2(
        &self,
        lookupblock: &mut LookupBlock,
        gpos1f2: SinglePosFormat2,
    ) -> Result<(), ReadError> {
        let input = self.resolve_coverage(&gpos1f2.coverage()?);
        let offset_data = gpos1f2.offset_data();
        for (glyph, vr) in input
            .into_iter()
            .zip(gpos1f2.value_records().iter().flatten())
        {
            let vr = self.resolve_value_record(&vr, offset_data)?;
            let statement =
                SinglePosStatement::new(vec![], vec![], vec![(glyph, Some(vr))], false, 0..0);
            lookupblock.statements.push(Statement::SinglePos(statement));
        }
        Ok(())
    }

    fn uncompile_gpos2_format1(
        &self,
        lookupblock: &mut LookupBlock,
        gpos2f1: PairPosFormat1,
    ) -> Result<(), ReadError> {
        let coverage = self.resolve_coverage(&gpos2f1.coverage()?);
        let mut statements = vec![];
        for (glyph, pairset) in coverage
            .into_iter()
            .zip(gpos2f1.pair_sets().iter().flatten())
        {
            let offset_data = pairset.offset_data();
            for pair in pairset.pair_value_records().iter().flatten() {
                let second_glyph = self.get_name(pair.second_glyph());
                let vr1 = self.resolve_value_record(pair.value_record1(), offset_data)?;
                let vr2 = self.resolve_value_record(pair.value_record2(), offset_data)?;
                statements.push(Statement::PairPos(PairPosStatement::new(
                    glyph.clone(),
                    GlyphContainer::GlyphName(second_glyph),
                    vr1.clone(),
                    if vr2.is_some() {
                        Some(vr2.clone())
                    } else {
                        None
                    },
                    false,
                    0..0,
                )));
            }
        }
        lookupblock.statements.extend(statements);
        Ok(())
    }

    fn uncompile_gpos2_format2(
        &self,
        lookupblock: &mut LookupBlock,
        gpos2f2: PairPosFormat2,
    ) -> Result<(), ReadError> {
        let class_def1 = gpos2f2.class_def1()?;
        let mut classes1: HashMap<u16, Vec<GlyphContainer>> = HashMap::new();
        for gid in gpos2f2.coverage()?.iter() {
            classes1
                .entry(class_def1.get(gid))
                .or_default()
                .push(GlyphContainer::GlyphName(self.get_name(gid)));
        }
        let mut classes2: HashMap<u16, Vec<GlyphContainer>> = HashMap::new();
        for (gid, class) in gpos2f2.class_def2()?.iter() {
            if class != 0 {
                classes2
                    .entry(class)
                    .or_default()
                    .push(GlyphContainer::GlyphName(self.get_name(gid)));
            }
        }
        let offset_data = gpos2f2.offset_data();
        for (class1, record) in gpos2f2.class1_records().iter().enumerate() {
            let Ok(record) = record else { continue };
            if !classes1.contains_key(&(class1 as u16)) {
                continue;
            }
            for (class2, subrecord) in record.class2_records().iter().enumerate() {
                if !classes2.contains_key(&(class2 as u16)) {
                    continue;
                }
                let Ok(subrecord) = subrecord else { continue };

                let vr1 = self.resolve_value_record(subrecord.value_record1(), offset_data)?;
                let vr2 = self.resolve_value_record(subrecord.value_record2(), offset_data)?;
                let statement = Statement::PairPos(PairPosStatement::new(
                    GlyphContainer::GlyphClass(GlyphClass::new(
                        classes1.get(&(class1 as u16)).cloned().unwrap_or_default(),
                        0..0,
                    )),
                    GlyphContainer::GlyphClass(GlyphClass::new(
                        classes2.get(&(class2 as u16)).cloned().unwrap_or_default(),
                        0..0,
                    )),
                    vr1.clone(),
                    if vr2.is_some() {
                        Some(vr2.clone())
                    } else {
                        None
                    },
                    false,
                    0..0,
                ));
                lookupblock.statements.push(statement);
            }
        }
        Ok(())
    }

    fn uncompile_gpos3(
        &mut self,
        lookupblock: &mut LookupBlock,
        gpos3: CursivePosFormat1,
    ) -> Result<(), ReadError> {
        let coverage = gpos3
            .coverage()?
            .iter()
            .map(|g| self.get_name(g))
            .collect::<Vec<_>>();
        for (glyph, entry) in coverage.into_iter().zip(gpos3.entry_exit_record()) {
            let entry_anchor = entry
                .entry_anchor(gpos3.offset_data())
                .transpose()?
                .map(|a| self.resolve_anchor(&a))
                .transpose()?;
            let exit_anchor = entry
                .exit_anchor(gpos3.offset_data())
                .transpose()?
                .map(|a| self.resolve_anchor(&a))
                .transpose()?;
            if let Some(ref entry_anchor) = entry_anchor {
                self.register_anchor(&glyph.name, entry_anchor, Some("entry"));
            }
            if let Some(ref exit_anchor) = exit_anchor {
                self.register_anchor(&glyph.name, exit_anchor, Some("exit"));
            }
            let statement = Statement::CursivePos(CursivePosStatement::new(
                GlyphContainer::GlyphName(glyph),
                entry_anchor,
                exit_anchor,
                0..0,
            ));
            lookupblock.statements.push(statement);
        }
        Ok(())
    }

    fn uncompile_gpos4(
        &mut self,
        lookupblock: &mut LookupBlock,
        gpos4: MarkBasePosFormat1,
    ) -> Result<(), ReadError> {
        let mark_coverage = self.resolve_coverage(&gpos4.mark_coverage()?);
        let base_coverage = gpos4
            .base_coverage()?
            .iter()
            .map(|g| self.get_name(g))
            .collect::<Vec<_>>();
        let base_array = gpos4.base_array()?;
        let mark_array = gpos4.mark_array()?;
        let mut base_anchor_rows = vec![];
        for (base_glyph, base_record) in base_coverage
            .iter()
            .zip(base_array.base_records().iter().flatten())
        {
            let anchors = base_record
                .base_anchors(base_array.offset_data())
                .iter()
                .map(|anchor| {
                    anchor
                        .transpose()?
                        .map(|anchor| self.resolve_anchor(&anchor))
                        .transpose()
                })
                .collect::<Result<Vec<_>, ReadError>>()?;
            base_anchor_rows.push((base_glyph.name.clone(), anchors));
        }
        let mark_class_to_base_glyph_anchor = Self::build_class_anchor_map(base_anchor_rows);

        let class_to_anchor_name = self.prepare_guessed_mark_classes(
            mark_coverage,
            mark_array,
            &mark_class_to_base_glyph_anchor,
        )?;

        // Emit the actual mark-base attachments
        for (base_glyph, base_record) in base_coverage
            .into_iter()
            .zip(base_array.base_records().iter().flatten())
        {
            let base_anchors = base_record
                .base_anchors(base_array.offset_data())
                .iter()
                .map(|anchor| {
                    anchor
                        .transpose()?
                        .map(|anchor| self.resolve_anchor(&anchor))
                        .transpose()
                })
                .collect::<Result<Vec<_>, ReadError>>()?;
            // Keep NULL anchors for bases with no anchors and classes with no base anchors.
            let no_anchors = base_anchors.iter().all(Option::is_none);
            let anchors_mark_classes = self.materialize_anchor_mark_classes(
                &base_glyph.name,
                &base_anchors,
                &class_to_anchor_name,
                |class| no_anchors || !mark_class_to_base_glyph_anchor.contains_key(&class),
            );
            let statement = Statement::MarkBasePos(MarkBasePosStatement::new(
                GlyphContainer::GlyphName(base_glyph.clone()),
                anchors_mark_classes,
                0..0,
            ));
            lookupblock.statements.push(statement);
        }
        Ok(())
    }

    fn gather_mark_classes(
        &mut self,
        mark_coverage: Vec<GlyphContainer>,
        mark_array: skrifa::raw::tables::gpos::MarkArray<'_>,
    ) -> Result<IndexMap<u16, Vec<(GlyphContainer, FeaAnchor)>>, ReadError> {
        let mut mark_classes: IndexMap<u16, Vec<(GlyphContainer, FeaAnchor)>> = IndexMap::new();
        for (mark_glyph, mark_record) in mark_coverage.iter().zip(mark_array.mark_records()) {
            let mark_anchor = mark_record.mark_anchor(mark_array.offset_data())?;
            let mark_class = mark_record.mark_class();
            let mark_anchor = self.resolve_anchor(&mark_anchor)?;
            mark_classes
                .entry(mark_class)
                .or_default()
                .push((mark_glyph.clone(), mark_anchor.clone()));
        }
        Ok(mark_classes)
    }

    fn prepare_guessed_mark_classes(
        &mut self,
        mark_coverage: Vec<GlyphContainer>,
        mark_array: skrifa::raw::tables::gpos::MarkArray<'_>,
        mark_class_to_target_glyph_anchor: &IndexMap<u16, IndexMap<SmolStr, FeaAnchor>>,
    ) -> Result<IndexMap<u16, SmolStr>, ReadError> {
        let mark_classes = self.gather_mark_classes(mark_coverage, mark_array)?;
        Ok(self.register_guessed_mark_classes(mark_classes, mark_class_to_target_glyph_anchor))
    }

    fn register_guessed_mark_classes(
        &mut self,
        mark_classes: IndexMap<u16, Vec<(GlyphContainer, FeaAnchor)>>,
        mark_class_to_target_glyph_anchor: &IndexMap<u16, IndexMap<SmolStr, FeaAnchor>>,
    ) -> IndexMap<u16, SmolStr> {
        let guessed_names = self.guess_anchor_names(mark_class_to_target_glyph_anchor);
        let mut guessed_name_by_class: IndexMap<u16, SmolStr> = IndexMap::new();
        for (class_number, name) in mark_class_to_target_glyph_anchor
            .keys()
            .cloned()
            .zip(guessed_names)
        {
            guessed_name_by_class.insert(class_number, name);
        }

        let mut class_to_anchor_name: IndexMap<u16, SmolStr> = IndexMap::new();
        for (class_number, mark_class) in mark_classes {
            let anchor_name = guessed_name_by_class
                .get(&class_number)
                .cloned()
                .unwrap_or_else(|| self.gensym(&format!("mark_class_{}", class_number)));
            self.register_mark_class(mark_class, &anchor_name);
            class_to_anchor_name.insert(class_number, anchor_name);
        }
        class_to_anchor_name
    }

    fn build_class_anchor_map(
        target_records: impl IntoIterator<Item = (SmolStr, Vec<Option<FeaAnchor>>)>,
    ) -> IndexMap<u16, IndexMap<SmolStr, FeaAnchor>> {
        let mut mark_class_to_target_glyph_anchor: IndexMap<u16, IndexMap<SmolStr, FeaAnchor>> =
            IndexMap::new();
        for (glyph_name, class_anchors) in target_records {
            for (class_number, anchor) in class_anchors.into_iter().enumerate() {
                if let Some(anchor) = anchor {
                    mark_class_to_target_glyph_anchor
                        .entry(class_number as u16)
                        .or_default()
                        .entry(glyph_name.clone())
                        .or_insert(anchor);
                }
            }
        }
        mark_class_to_target_glyph_anchor
    }

    /// Omit NULL anchors unless `keep_null` selects their class; compilers fill in the rest.
    fn materialize_anchor_mark_classes(
        &mut self,
        target_glyph_name: &SmolStr,
        class_anchors: &[Option<FeaAnchor>],
        class_to_anchor_name: &IndexMap<u16, SmolStr>,
        keep_null: impl Fn(u16) -> bool,
    ) -> Vec<(Option<FeaAnchor>, MarkClass)> {
        let mut anchors_mark_classes = vec![];
        for (class_number, anchor) in class_anchors.iter().enumerate() {
            let Some(anchor_name) = class_to_anchor_name.get(&(class_number as u16)) else {
                continue;
            };
            match anchor {
                Some(anchor) => self.register_anchor(target_glyph_name, anchor, Some(anchor_name)),
                None if !keep_null(class_number as u16) => continue,
                None => {}
            }
            anchors_mark_classes.push((anchor.clone(), MarkClass::new(anchor_name)));
        }
        anchors_mark_classes
    }

    fn uncompile_gpos5(
        &mut self,
        lookupblock: &mut LookupBlock,
        gpos5: MarkLigPosFormat1,
    ) -> Result<(), ReadError> {
        let mark_coverage = self.resolve_coverage(&gpos5.mark_coverage()?);
        let ligature_coverage = gpos5
            .ligature_coverage()?
            .iter()
            .map(|g| self.get_name(g))
            .collect::<Vec<_>>();
        let mark_array = gpos5.mark_array()?;
        let ligature_array = gpos5.ligature_array()?;
        let mut ligature_anchor_rows = vec![];
        for (ligature_glyph, ligature_attach) in ligature_coverage
            .iter()
            .zip(ligature_array.ligature_attaches().iter().flatten())
        {
            for component_record in ligature_attach.component_records().iter().flatten() {
                let anchors = component_record
                    .ligature_anchors(ligature_attach.offset_data())
                    .iter()
                    .map(|anchor| {
                        anchor
                            .transpose()?
                            .map(|anchor| self.resolve_anchor(&anchor))
                            .transpose()
                    })
                    .collect::<Result<Vec<_>, ReadError>>()?;
                ligature_anchor_rows.push((ligature_glyph.name.clone(), anchors));
            }
        }
        let mark_class_to_ligature_glyph_anchor =
            Self::build_class_anchor_map(ligature_anchor_rows);

        let class_to_anchor_name = self.prepare_guessed_mark_classes(
            mark_coverage,
            mark_array,
            &mark_class_to_ligature_glyph_anchor,
        )?;

        for (ligature_glyph, ligature_attach) in ligature_coverage
            .into_iter()
            .zip(ligature_array.ligature_attaches().iter().flatten())
        {
            let mut components_anchors_mark_classes = vec![];
            for component_record in ligature_attach.component_records().iter().flatten() {
                let class_anchors = component_record
                    .ligature_anchors(ligature_attach.offset_data())
                    .iter()
                    .map(|anchor| {
                        anchor
                            .transpose()?
                            .map(|anchor| self.resolve_anchor(&anchor))
                            .transpose()
                    })
                    .collect::<Result<Vec<_>, ReadError>>()?;
                // Keep NULL anchors for classes with no ligature anchors.
                // Components with no anchors are written as a bare NULL anchor.
                let anchors_mark_classes = self.materialize_anchor_mark_classes(
                    &ligature_glyph.name,
                    &class_anchors,
                    &class_to_anchor_name,
                    |class| !mark_class_to_ligature_glyph_anchor.contains_key(&class),
                );
                components_anchors_mark_classes.push(anchors_mark_classes);
            }

            let statement = Statement::MarkLigPos(MarkLigPosStatement::new(
                GlyphContainer::GlyphName(ligature_glyph),
                components_anchors_mark_classes,
                0..0,
            ));
            lookupblock.statements.push(statement);
        }

        Ok(())
    }
    fn uncompile_gpos6(
        &mut self,
        lookupblock: &mut LookupBlock,
        gpos6: MarkMarkPosFormat1,
    ) -> Result<(), ReadError> {
        let mark1_coverage = self.resolve_coverage(&gpos6.mark1_coverage()?);
        let mark2_coverage = gpos6
            .mark2_coverage()?
            .iter()
            .map(|g| self.get_name(g))
            .collect::<Vec<_>>();
        let mark1_array = gpos6.mark1_array()?;
        let mark2_array = gpos6.mark2_array()?;
        let mut mark2_anchor_rows = vec![];
        for (mark2_glyph, mark2_record) in mark2_coverage
            .iter()
            .zip(mark2_array.mark2_records().iter().flatten())
        {
            let anchors = mark2_record
                .mark2_anchors(mark2_array.offset_data())
                .iter()
                .map(|anchor| {
                    anchor
                        .transpose()?
                        .map(|anchor| self.resolve_anchor(&anchor))
                        .transpose()
                })
                .collect::<Result<Vec<_>, ReadError>>()?;
            mark2_anchor_rows.push((mark2_glyph.name.clone(), anchors));
        }
        let mark_class_to_mark2_glyph_anchor = Self::build_class_anchor_map(mark2_anchor_rows);

        let class_to_anchor_name = self.prepare_guessed_mark_classes(
            mark1_coverage,
            mark1_array,
            &mark_class_to_mark2_glyph_anchor,
        )?;

        for (mark2_glyph, mark2_record) in mark2_coverage
            .into_iter()
            .zip(mark2_array.mark2_records().iter().flatten())
        {
            let class_anchors = mark2_record
                .mark2_anchors(mark2_array.offset_data())
                .iter()
                .map(|anchor| {
                    anchor
                        .transpose()?
                        .map(|anchor| self.resolve_anchor(&anchor))
                        .transpose()
                })
                .collect::<Result<Vec<_>, ReadError>>()?;

            // Keep NULL anchors for target marks with no anchors and classes with no target anchors.
            let no_anchors = class_anchors.iter().all(Option::is_none);
            let anchors_mark_classes = self.materialize_anchor_mark_classes(
                &mark2_glyph.name,
                &class_anchors,
                &class_to_anchor_name,
                |class| no_anchors || !mark_class_to_mark2_glyph_anchor.contains_key(&class),
            );

            let statement = Statement::MarkMarkPos(MarkMarkPosStatement::new(
                GlyphContainer::GlyphName(mark2_glyph),
                anchors_mark_classes,
                0..0,
            ));
            lookupblock.statements.push(statement);
        }

        Ok(())
    }

    fn guess_anchor_names(
        &mut self,
        mark_class_to_base_glyph_anchor: &IndexMap<u16, IndexMap<SmolStr, FeaAnchor>>,
    ) -> Vec<SmolStr> {
        let mut new_names = vec![];
        for (class_number, base_glyphs_anchors) in mark_class_to_base_glyph_anchor {
            let mut xs = vec![];
            let mut ys = vec![];
            for (base_glyph, anchor) in base_glyphs_anchors {
                let Some(gid) = self.glyph_name_to_id.get(base_glyph).cloned() else {
                    continue;
                };
                let bounds = self.glyph_metrics.bounds(gid).unwrap_or_default();
                let width = self.glyph_metrics.advance_width(gid).unwrap_or_default();
                let height = bounds.y_max - bounds.y_min;
                let (x, y) = anchor_location(anchor);
                let x_percentage = if width > 0.0 {
                    (x - bounds.x_min) / width
                } else {
                    0.5
                };
                let y_percentage = if height > 0.0 {
                    (y - bounds.y_min) / height
                } else {
                    0.5
                };
                xs.push(x_percentage);
                ys.push(y_percentage);
            }
            // Now guess: if they're all majority in top, topright, topleft, bottom, bottomright, bottomleft, center, etc
            // in order if not already registered in the "anchors" field.
            if let Some(name) = majority_in_quadrant(&xs, &ys)
                && self.anchors.get(name).is_none()
            {
                new_names.push(name.into());
                self.anchors
                    .insert(name.into(), base_glyphs_anchors.clone());
                continue;
            }
            // Create one with a symbol
            let name = self.gensym(&format!("mark_class_{}", class_number));
            self.anchors
                .insert(name.clone(), base_glyphs_anchors.clone());
            new_names.push(name);
        }
        new_names
    }
}

fn anchor_location(anchor: &FeaAnchor) -> (f32, f32) {
    let x = match &anchor.x {
        Metric::Scalar(s) => *s as f32,
        Metric::Variable(items) => items
            .first()
            .map(|(_loc, value)| *value as f32)
            .unwrap_or(0.0),
        Metric::GlyphsAppNumber(_) => 0.0, // You deserve to lose
    };
    let y = match &anchor.y {
        Metric::Scalar(s) => *s as f32,
        Metric::Variable(items) => items
            .first()
            .map(|(_loc, value)| *value as f32)
            .unwrap_or(0.0),
        Metric::GlyphsAppNumber(_) => 0.0,
    };
    (x, y)
}

fn majority_in_quadrant(xs: &[f32], ys: &[f32]) -> Option<&'static str> {
    let mut counts = HashMap::new();
    for (x, y) in xs.iter().zip(ys) {
        let quadrant = if *x < 0.33 && *y > 0.66 {
            "topleft"
        } else if *x > 0.66 && *y > 0.66 {
            "topright"
        } else if *x < 0.33 && *y < 0.33 {
            "bottomleft"
        } else if *x > 0.66 && *y < 0.33 {
            "bottomright"
        } else if *y > 0.66 {
            "top"
        } else if *y < 0.33 {
            "bottom"
        } else if *x < 0.33 {
            "left"
        } else if *x > 0.66 {
            "right"
        } else if (*x >= 0.33 && *x <= 0.66) && (*y >= 0.33 && *y <= 0.66) {
            "center"
        } else {
            continue;
        };
        *counts.entry(quadrant).or_insert(0) += 1;
    }
    // If there's a clear winner, report it
    let total = xs.len() as f32;
    counts.into_iter().find_map(|(quadrant, count)| {
        if count as f32 / total > 0.5 {
            Some(quadrant)
        } else {
            None
        }
    })
}

fn to_context_statement(statement: ChainedContextStatement<Pos>) -> Statement {
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
