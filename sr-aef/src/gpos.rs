use std::collections::HashMap;

use crate::{SimpleUserLocation, UncompileContext};
use fea_rs_ast::{
    Anchor as FeaAnchor, CursivePosStatement, GlyphClass, GlyphContainer, LookupBlock,
    MarkBasePosStatement, MarkClass, MarkLigPosStatement, MarkMarkPosStatement, Metric,
    PairPosStatement, Pos, SinglePosStatement, Statement, ValueRecord as FeaValueRecord,
};
use indexmap::IndexMap;
use skrifa::raw::{
    ReadError,
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
        let gpos_lookup_list: LookupList<PositionLookup> = match self.gpos.as_ref() {
            Some(gpos) => gpos.lookup_list()?,
            None => return Ok(()),
        };
        for (i, lookup) in gpos_lookup_list.lookups().iter().flatten().enumerate() {
            let subtables = lookup.subtables()?;
            match subtables {
                PositionSubtables::Single(subtables) => {
                    let mut lookupblock =
                        self.create_next_lookup_block("gpos_single", i as u16, Pos);
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
                    let mut lookupblock = self.create_next_lookup_block("gpos_pair", i as u16, Pos);
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
                    let mut lookupblock =
                        self.create_next_lookup_block("gpos_cursive", i as u16, Pos);
                    for subtable in subtables.iter().flatten() {
                        self.uncompile_gpos3(&mut lookupblock, subtable)?;
                    }
                    self.lookups.insert(lookupblock.name.clone(), lookupblock);
                }
                PositionSubtables::MarkToBase(subtables) => {
                    let mut lookupblock =
                        self.create_next_lookup_block("gpos_mark_to_base", i as u16, Pos);
                    for subtable in subtables.iter().flatten() {
                        self.uncompile_gpos4(&mut lookupblock, subtable)?;
                    }
                    self.lookups.insert(lookupblock.name.clone(), lookupblock);
                }
                PositionSubtables::MarkToLig(subtables) => {
                    let mut lookupblock =
                        self.create_next_lookup_block("gpos_mark_to_ligature", i as u16, Pos);
                    for subtable in subtables.iter().flatten() {
                        self.uncompile_gpos5(&mut lookupblock, subtable)?;
                    }
                    self.lookups.insert(lookupblock.name.clone(), lookupblock);
                }
                PositionSubtables::MarkToMark(subtables) => {
                    let mut lookupblock =
                        self.create_next_lookup_block("gpos_mark_to_mark", i as u16, Pos);
                    for subtable in subtables.iter().flatten() {
                        self.uncompile_gpos6(&mut lookupblock, subtable)?;
                    }
                    self.lookups.insert(lookupblock.name.clone(), lookupblock);
                }
                PositionSubtables::Contextual(subtables) => {
                    let mut lookupblock =
                        self.create_next_lookup_block("gpos_contextual", i as u16, Pos);
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
                    let mut lookupblock =
                        self.create_next_lookup_block("gpos_chain_contextual", i as u16, Pos);
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

    fn resolve_value_record(&self, value_record: &ValueRecord) -> FeaValueRecord {
        let x_placement = value_record.x_placement().map(Metric::from);
        let y_placement = value_record.y_placement().map(Metric::from);
        let x_advance = value_record.x_advance().map(Metric::from);
        let y_advance = value_record.y_advance().map(Metric::from);
        FeaValueRecord::new(
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
        )
    }

    fn resolve_anchor(&self, anchor: &AnchorTable) -> Result<FeaAnchor, ReadError> {
        let mut x_variations: Vec<(SimpleUserLocation, i16)> = Vec::new();
        let mut y_variations: Vec<(SimpleUserLocation, i16)> = Vec::new();
        if let Some(ivs) = self.variation_store()? {
            x_variations =
                self.resolve_variation_index(anchor.x_coordinate(), anchor.x_device(), &ivs)?;
            y_variations =
                self.resolve_variation_index(anchor.y_coordinate(), anchor.y_device(), &ivs)?;
        }
        let x = if x_variations.len() < 2 {
            // we always have the default
            Metric::Scalar(anchor.x_coordinate())
        } else {
            Metric::Variable(x_variations)
        };
        let y = if y_variations.len() < 2 {
            Metric::Scalar(anchor.y_coordinate())
        } else {
            Metric::Variable(y_variations)
        };

        Ok(FeaAnchor::new(x, y, None, None, None, None, 0..0))
    }

    fn uncompile_gpos1_format1(
        &self,
        lookupblock: &mut LookupBlock,
        gpos1f1: SinglePosFormat1,
    ) -> Result<(), ReadError> {
        let input = self.resolve_coverage(&gpos1f1.coverage()?);
        let vr = self.resolve_value_record(&gpos1f1.value_record());
        let statement = SinglePosStatement::new(
            vec![],
            vec![],
            input
                .into_iter()
                .map(|gid| (gid, Some(vr.clone())))
                .collect(),
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
        let statement = SinglePosStatement::new(
            vec![],
            vec![],
            input
                .into_iter()
                .zip(
                    gpos1f2
                        .value_records()
                        .iter()
                        .flatten()
                        .map(|vr| self.resolve_value_record(&vr)),
                )
                .map(|(gid, vr)| (gid, Some(vr)))
                .collect(),
            false,
            0..0,
        );
        lookupblock.statements.push(Statement::SinglePos(statement));
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
            for pair in pairset.pair_value_records().iter().flatten() {
                let second_glyph = self.get_name(pair.second_glyph());
                let vr1 = self.resolve_value_record(pair.value_record1());
                let vr2 = self.resolve_value_record(pair.value_record2());
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
        let classes1 = self.resolve_classes(gpos2f2.class_def1()?);
        let classes2 = self.resolve_classes(gpos2f2.class_def2()?);
        for (class1, record) in gpos2f2.class1_records().iter().enumerate() {
            let Ok(record) = record else { continue };
            for (class2, subrecord) in record.class2_records().iter().enumerate() {
                let Ok(subrecord) = subrecord else { continue };

                let vr1 = self.resolve_value_record(subrecord.value_record1());
                let vr2 = self.resolve_value_record(subrecord.value_record2());
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
        let mut mark_classes: IndexMap<u16, Vec<(GlyphContainer, FeaAnchor)>> = IndexMap::new();
        // Emit mark classes first
        for (mark_glyph, mark_record) in mark_coverage.iter().zip(mark_array.mark_records()) {
            let mark_anchor = mark_record.mark_anchor(mark_array.offset_data())?;
            let mark_class = mark_record.mark_class();
            let mark_anchor = self.resolve_anchor(&mark_anchor)?;
            mark_classes
                .entry(mark_class)
                .or_default()
                .push((mark_glyph.clone(), mark_anchor.clone()));
        }

        // let class_to_anchor_name: Vec<SmolStr> = self.register_mark_classes(mark_classes);

        let mut mark_class_to_base_glyph_anchor: IndexMap<u16, IndexMap<SmolStr, FeaAnchor>> =
            IndexMap::new();
        for (base_glyph, base_record) in base_coverage
            .iter()
            .zip(base_array.base_records().iter().flatten())
        {
            let base_anchors = base_record.base_anchors(base_array.offset_data());
            for (class_number, base_anchor) in base_anchors.iter().enumerate() {
                if let Some(base_anchor) = base_anchor.transpose()? {
                    let base_anchor = self.resolve_anchor(&base_anchor)?;
                    mark_class_to_base_glyph_anchor
                        .entry(class_number as u16)
                        .or_default()
                        .insert(base_glyph.name.clone(), base_anchor.clone());
                }
            }
        }

        let class_to_anchor_name: Vec<SmolStr> =
            self.guess_anchor_names(&mark_class_to_base_glyph_anchor);

        for (base_glyph, base_record) in base_coverage
            .into_iter()
            .zip(base_array.base_records().iter().flatten())
        {
            let base_anchors = base_record.base_anchors(base_array.offset_data());
            let mut anchors_mark_classes = vec![];
            for (base_anchor, anchor_name) in base_anchors.iter().zip(class_to_anchor_name.iter()) {
                if let Some(base_anchor) = base_anchor.transpose()? {
                    let base_anchor = self.resolve_anchor(&base_anchor)?;
                    self.register_anchor(&base_glyph.name, &base_anchor, Some(anchor_name));
                    anchors_mark_classes.push((base_anchor, MarkClass::new(anchor_name)));
                }
            }
            let statement = Statement::MarkBasePos(MarkBasePosStatement::new(
                GlyphContainer::GlyphName(base_glyph.clone()),
                anchors_mark_classes,
                0..0,
            ));
            lookupblock.statements.push(statement);
        }
        Ok(())
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

        let mut mark_classes: IndexMap<u16, Vec<(GlyphContainer, FeaAnchor)>> = IndexMap::new();
        for (mark_glyph, mark_record) in mark_coverage.into_iter().zip(mark_array.mark_records()) {
            let mark_anchor = mark_record.mark_anchor(mark_array.offset_data())?;
            let mark_class = mark_record.mark_class();
            let mark_anchor = self.resolve_anchor(&mark_anchor)?;
            mark_classes
                .entry(mark_class)
                .or_default()
                .push((mark_glyph, mark_anchor.clone()));
        }
        let class_to_anchor_name: Vec<SmolStr> = self.register_mark_classes(mark_classes);

        for (ligature_glyph, ligature_attach) in ligature_coverage
            .into_iter()
            .zip(ligature_array.ligature_attaches().iter().flatten())
        {
            let mut components_anchors_mark_classes = vec![];
            for component_record in ligature_attach.component_records().iter().flatten() {
                let mut anchors_mark_classes = vec![];
                for (class_number, ligature_anchor) in component_record
                    .ligature_anchors(ligature_attach.offset_data())
                    .iter()
                    .enumerate()
                {
                    if let Some(ligature_anchor) = ligature_anchor.transpose()? {
                        let ligature_anchor = self.resolve_anchor(&ligature_anchor)?;
                        let anchor_name = class_to_anchor_name
                            .get(class_number)
                            .cloned()
                            .unwrap_or_else(|| format!("mark_class_{}", class_number).into());
                        self.register_anchor(
                            &ligature_glyph.name,
                            &ligature_anchor,
                            Some(&anchor_name),
                        );
                        anchors_mark_classes.push((ligature_anchor, MarkClass::new(&anchor_name)));
                    }
                }
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

        let mut mark_classes: IndexMap<u16, Vec<(GlyphContainer, FeaAnchor)>> = IndexMap::new();
        for (mark_glyph, mark_record) in mark1_coverage.into_iter().zip(mark1_array.mark_records())
        {
            let mark_anchor = mark_record.mark_anchor(mark1_array.offset_data())?;
            let mark_class = mark_record.mark_class();
            let mark_anchor = self.resolve_anchor(&mark_anchor)?;
            mark_classes
                .entry(mark_class)
                .or_default()
                .push((mark_glyph, mark_anchor.clone()));
        }
        let class_to_anchor_name: Vec<SmolStr> = self.register_mark_classes(mark_classes);

        for (mark2_glyph, mark2_record) in mark2_coverage
            .into_iter()
            .zip(mark2_array.mark2_records().iter().flatten())
        {
            let mut anchors_mark_classes = vec![];
            for (class_number, mark2_anchor) in mark2_record
                .mark2_anchors(mark2_array.offset_data())
                .iter()
                .enumerate()
            {
                if let Some(mark2_anchor) = mark2_anchor.transpose()? {
                    let mark2_anchor = self.resolve_anchor(&mark2_anchor)?;
                    let anchor_name = class_to_anchor_name
                        .get(class_number)
                        .cloned()
                        .unwrap_or_else(|| format!("mark_class_{}", class_number).into());
                    self.register_anchor(&mark2_glyph.name, &mark2_anchor, Some(&anchor_name));
                    anchors_mark_classes.push((mark2_anchor, MarkClass::new(&anchor_name)));
                }
            }

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
                let width = bounds.x_max - bounds.x_min;
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
        let quadrant = if *x < 0.5 && *y > 0.5 {
            "topleft"
        } else if *x > 0.5 && *y > 0.5 {
            "topright"
        } else if *x < 0.5 && *y < 0.5 {
            "bottomleft"
        } else if *x > 0.5 && *y < 0.5 {
            "bottomright"
        } else if *y > 0.5 {
            "top"
        } else if *y < 0.5 {
            "bottom"
        } else if *x < 0.5 {
            "left"
        } else if *x > 0.5 {
            "right"
        } else {
            "center"
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
