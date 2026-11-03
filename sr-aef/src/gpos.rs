//! Un-compiling of GPOS lookups.

use fea_rs_ast::AsFea;
use std::collections::{HashMap, HashSet};

use fea_rs_ast::{
    Anchor as FeaAnchor, CursivePosStatement, GlyphClass, GlyphContainer, LookupBlock,
    MarkBasePosStatement, MarkClass, MarkLigPosStatement, MarkMarkPosStatement, Metric,
    PairPosStatement, Pos, SinglePosStatement, Statement, ValueRecord as FeaValueRecord,
};
use indexmap::IndexMap;
use skrifa::{
    GlyphId,
    raw::{
        ReadError,
        tables::gpos::{
            AnchorTable, CursivePos, ExtensionPosFormat1, MarkBasePos, MarkLigPos, MarkMarkPos,
            PairPos, PositionChainContext, PositionLookup, PositionSequenceContext,
            PositionSubtables, SinglePos, ValueFormat, ValueRecord,
        },
    },
};
use smol_str::SmolStr;

use crate::UncompileContext;
use crate::contextual::to_context_statement;
use crate::layout::{
    AnchorRow, CursivePosSource, LookupKind, MarkBasePosSource, MarkLigPosSource,
    MarkMarkPosSource, PairPosSource, PairPositioning, SinglePosSource, SinglePosValues,
    UncompileSubtables, impl_uncompile_subtables,
};

/// The anchor of one component of a mark attachment target, with the mark
/// class it attaches to.
type AnchoredMarks = Vec<(Option<FeaAnchor>, MarkClass)>;

/// Every target of a mark attachment lookup, with its rows of anchors.
type MarkAttachment = Vec<(GlyphId, Vec<AnchoredMarks>)>;

impl<'a> UncompileContext<'a> {
    pub(crate) fn uncompile_gpos_lookups(&mut self) -> Result<(), ReadError> {
        let Some(lookup_list) = self.gpos.as_ref().and_then(|gpos| gpos.lookup_list().ok()) else {
            return Ok(());
        };
        // Name every lookup first: contextual lookups refer to lookups which
        // may come later in the list.
        for (i, lookup) in lookup_list.lookups().iter().flatten().enumerate() {
            self.assign_lookup_name(lookup.subtables()?.prefix(), i as u16, Pos);
        }
        for (i, lookup) in lookup_list.lookups().iter().flatten().enumerate() {
            let subtables = lookup.subtables()?;
            // a pair lookup's varying values are stated at all of its regions
            let regions = match &subtables {
                PositionSubtables::Pair(pairs) => {
                    Some(self.pair_lookup_regions(pairs.iter().flatten())?)
                }
                _ => None,
            };
            *self.lookup_regions.borrow_mut() = regions;
            let mut block = subtables.uncompile(self, i as u16, Pos)?;
            *self.lookup_regions.borrow_mut() = None;
            if let Some(Statement::Subtable(_)) = block.statements.last() {
                block.statements.pop();
            }
            block.use_extension = matches!(lookup, PositionLookup::Extension(_));
            self.add_lookup_flags(
                &mut block,
                lookup.lookup_flag(),
                lookup.mark_filtering_set(),
            );
            self.lookups.insert(block.name.clone(), block);
        }
        Ok(())
    }

    fn resolve_value_record(
        &self,
        value_record: &ValueRecord,
    ) -> Result<FeaValueRecord, ReadError> {
        let x_placement = value_record
            .x_placement()
            .map(|vr| self.resolve_pos_with_variations(vr, value_record.x_placement_device()))
            .transpose()?;
        let y_placement = value_record
            .y_placement()
            .map(|vr| self.resolve_pos_with_variations(vr, value_record.y_placement_device()))
            .transpose()?;
        let x_advance = value_record
            .x_advance()
            .map(|vr| self.resolve_pos_with_variations(vr, value_record.x_advance_device()))
            .transpose()?;
        let y_advance = value_record
            .y_advance()
            .map(|vr| self.resolve_pos_with_variations(vr, value_record.y_advance_device()))
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

    fn uncompile_single_pos(
        &mut self,
        block: &mut LookupBlock,
        subtable: &SinglePos<'a>,
    ) -> Result<(), ReadError> {
        let inputs = self.resolve_coverage(&subtable.coverage()?);
        match subtable.values() {
            SinglePosValues::Shared(value) => {
                let value = self.resolve_value_record(&value)?;
                let statement = SinglePosStatement::new(
                    vec![],
                    vec![],
                    vec![(self.class_members_to_container(inputs), Some(value))],
                    false,
                    0..0,
                );
                block.statements.push(Statement::SinglePos(statement));
            }
            SinglePosValues::PerGlyph(values) => {
                let mut statements = vec![];
                for (glyph, value) in inputs.into_iter().zip(values) {
                    let value = self.resolve_value_record(&value)?;
                    statements.push(Statement::SinglePos(SinglePosStatement::new(
                        vec![],
                        vec![],
                        vec![(glyph, Some(value))],
                        false,
                        0..0,
                    )));
                }
                block.statements.extend(statements);
            }
        }
        Ok(())
    }

    fn uncompile_pair_pos(
        &mut self,
        block: &mut LookupBlock,
        subtable: &PairPos<'a>,
    ) -> Result<(), ReadError> {
        match subtable.positioning()? {
            PairPositioning::Pairs {
                coverage,
                pair_sets,
            } => {
                let mut statements = vec![];
                for (glyph, pair_set) in self.glyph_containers(&coverage).into_iter().zip(pair_sets)
                {
                    for pair in pair_set {
                        statements.push(self.pair_statement(
                            glyph.clone(),
                            GlyphContainer::GlyphName(self.get_name(pair.second)),
                            &pair.first,
                            &pair.first_is_next,
                        )?);
                    }
                }
                block.statements.extend(statements);
            }
            PairPositioning::Classes {
                coverage,
                class_def1,
                class_def2,
                records,
            } => {
                let mut classes1: HashMap<u32, Vec<GlyphContainer>> = HashMap::new();
                for glyph in &coverage {
                    classes1
                        .entry(class_def1.get(*glyph))
                        .or_default()
                        .push(GlyphContainer::GlyphName(self.get_name(*glyph)));
                }
                let mut classes2: HashMap<u32, Vec<GlyphContainer>> = HashMap::new();
                for (glyph, class) in class_def2.iter() {
                    if class != 0 {
                        classes2
                            .entry(class)
                            .or_default()
                            .push(GlyphContainer::GlyphName(self.get_name(glyph)));
                    }
                }
                // Class 0 of the second glyphs is every glyph no other class
                // has: a value against it kerns a first glyph with all of them
                let class_zero_kerns = records.iter().any(|record| {
                    record
                        .as_ref()
                        .and_then(|record| record.first())
                        .and_then(|zero| zero.as_ref())
                        .is_some_and(|zero| has_value(&zero.first) || has_value(&zero.second))
                });
                if class_zero_kerns {
                    let classed: HashSet<u32> = class_def2
                        .iter()
                        .filter(|(_, class)| *class != 0)
                        .map(|(glyph, _)| glyph.to_u32())
                        .collect();
                    let rest: Vec<GlyphContainer> = (0..self.num_glyphs as u32)
                        .filter(|gid| !classed.contains(gid))
                        .map(|gid| GlyphContainer::GlyphName(self.get_name(GlyphId::new(gid))))
                        .collect();
                    if !rest.is_empty() {
                        classes2.insert(0, rest);
                    }
                }
                // each class written once, by name where large
                let mut keys1: Vec<u32> = classes1.keys().copied().collect();
                keys1.sort();
                let mut containers1: HashMap<u32, GlyphContainer> = HashMap::new();
                for class in keys1 {
                    containers1.insert(class, self.pair_class(classes1.get(&class)));
                }
                let mut keys2: Vec<u32> = classes2.keys().copied().collect();
                keys2.sort();
                let mut containers2: HashMap<u32, GlyphContainer> = HashMap::new();
                for class in keys2 {
                    containers2.insert(class, self.pair_class(classes2.get(&class)));
                }
                for (class1, record) in records.iter().enumerate() {
                    let Some(record) = record else { continue };
                    let Some(first) = containers1.get(&(class1 as u32)) else {
                        continue;
                    };
                    // pairs that adjust nothing say nothing a missing pair does
                    // not, but one keeps the row's first glyphs covered (later
                    // subtables never see them); a dense class matrix is mostly
                    // zeros
                    let any_value = record.iter().enumerate().any(|(class2, value)| {
                        containers2.contains_key(&(class2 as u32))
                            && value.as_ref().is_some_and(|value| {
                                has_value(&value.first) || has_value(&value.second)
                            })
                    });
                    let mut row_written = false;
                    for (class2, value) in record.iter().enumerate() {
                        let Some(value) = value else { continue };
                        let Some(second) = containers2.get(&(class2 as u32)) else {
                            continue;
                        };
                        let zero = !has_value(&value.first) && !has_value(&value.second);
                        if zero && (any_value || row_written) {
                            continue;
                        }
                        row_written = true;
                        block.statements.push(self.pair_statement(
                            first.clone(),
                            second.clone(),
                            &value.first,
                            &value.second,
                        )?);
                    }
                }
            }
        }
        Ok(())
    }

    /// The variation regions any value of a pair lookup uses
    fn pair_lookup_regions(
        &self,
        subtables: impl Iterator<Item = PairPos<'a>>,
    ) -> Result<std::collections::BTreeSet<u16>, ReadError> {
        let mut regions = std::collections::BTreeSet::new();
        for subtable in subtables {
            match subtable.positioning()? {
                PairPositioning::Pairs { pair_sets, .. } => {
                    for pair in pair_sets.iter().flatten() {
                        for record in [&pair.first, &pair.first_is_next] {
                            regions.extend(self.value_record_regions(record)?);
                        }
                    }
                }
                PairPositioning::Classes { records, .. } => {
                    for value in records.iter().flatten().flatten().flatten() {
                        for record in [&value.first, &value.second] {
                            regions.extend(self.value_record_regions(record)?);
                        }
                    }
                }
            }
        }
        Ok(regions)
    }

    fn pair_statement(
        &mut self,
        first: GlyphContainer,
        second: GlyphContainer,
        value_record1: &ValueRecord,
        value_record2: &ValueRecord,
    ) -> Result<Statement, ReadError> {
        let vr1 = self.resolve_value_record(value_record1)?;
        let vr2 = self.resolve_value_record(value_record2)?;
        Ok(Statement::PairPos(PairPosStatement::new(
            first,
            second,
            vr1,
            if vr2.is_some() { Some(vr2) } else { None },
            false,
            0..0,
        )))
    }

    fn uncompile_cursive_pos(
        &mut self,
        block: &mut LookupBlock,
        subtable: &CursivePos<'a>,
    ) -> Result<(), ReadError> {
        let coverage = subtable.coverage()?;
        for (glyph, (entry, exit)) in coverage.iter().zip(subtable.anchors()?) {
            let glyph = self.get_name(glyph);
            let entry_anchor = entry
                .map(|anchor| self.resolve_anchor(&anchor))
                .transpose()?;
            let exit_anchor = exit
                .map(|anchor| self.resolve_anchor(&anchor))
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
            block.statements.push(statement);
        }
        Ok(())
    }

    fn uncompile_mark_base_pos(
        &mut self,
        block: &mut LookupBlock,
        subtable: &MarkBasePos<'a>,
    ) -> Result<(), ReadError> {
        let targets = subtable
            .base_coverage()?
            .iter()
            .zip(subtable.base_anchors()?)
            .map(|(glyph, anchors)| (glyph, vec![anchors]))
            .collect();
        for (glyph, mut components) in
            self.uncompile_mark_attachment(subtable.marks()?, targets, true)?
        {
            let statement = Statement::MarkBasePos(MarkBasePosStatement::new(
                GlyphContainer::GlyphName(self.get_name(glyph)),
                components.pop().unwrap_or_default(),
                0..0,
            ));
            block.statements.push(statement);
        }
        Ok(())
    }

    fn uncompile_mark_lig_pos(
        &mut self,
        block: &mut LookupBlock,
        subtable: &MarkLigPos<'a>,
    ) -> Result<(), ReadError> {
        let targets = subtable
            .ligature_coverage()?
            .iter()
            .zip(subtable.ligature_anchors()?)
            .collect();
        for (glyph, components) in
            self.uncompile_mark_attachment(subtable.marks()?, targets, false)?
        {
            let statement = Statement::MarkLigPos(MarkLigPosStatement::new(
                GlyphContainer::GlyphName(self.get_name(glyph)),
                components,
                0..0,
            ));
            block.statements.push(statement);
        }
        Ok(())
    }

    fn uncompile_mark_mark_pos(
        &mut self,
        block: &mut LookupBlock,
        subtable: &MarkMarkPos<'a>,
    ) -> Result<(), ReadError> {
        let targets = subtable
            .mark2_coverage()?
            .iter()
            .zip(subtable.mark2_anchors()?)
            .map(|(glyph, anchors)| (glyph, vec![anchors]))
            .collect();
        for (glyph, mut components) in
            self.uncompile_mark_attachment(subtable.marks()?, targets, true)?
        {
            let statement = Statement::MarkMarkPos(MarkMarkPosStatement::new(
                GlyphContainer::GlyphName(self.get_name(glyph)),
                components.pop().unwrap_or_default(),
                0..0,
            ));
            block.statements.push(statement);
        }
        Ok(())
    }

    /// The part shared by the three mark attachment lookups.
    ///
    /// Marks are grouped into classes and the classes are named (guessed from
    /// the anchors they attach to), then every target glyph is turned into a
    /// row of attached anchors per class.  Each target has one row per
    /// component: exactly one, except for ligatures, which have one per
    /// component of the ligature.
    ///
    /// `null_anchors_when_empty` keeps a `NULL` anchor for every class of a
    /// target which has no anchors at all, which is what mark-to-base and
    /// mark-to-mark need.
    fn uncompile_mark_attachment(
        &mut self,
        marks: Vec<(GlyphId, u16, AnchorTable<'a>)>,
        targets: Vec<(GlyphId, Vec<AnchorRow<'a>>)>,
        null_anchors_when_empty: bool,
    ) -> Result<MarkAttachment, ReadError> {
        let mut mark_classes: IndexMap<u16, Vec<(GlyphContainer, FeaAnchor)>> = IndexMap::new();
        for (glyph, class, anchor) in marks {
            let anchor = self.resolve_anchor(&anchor)?;
            mark_classes
                .entry(class)
                .or_default()
                .push((GlyphContainer::GlyphName(self.get_name(glyph)), anchor));
        }

        let mut resolved = vec![];
        for (glyph, components) in targets {
            let name = self.get_name(glyph).name;
            let mut rows = vec![];
            for component in components {
                rows.push(
                    component
                        .into_iter()
                        .map(|anchor| {
                            anchor
                                .map(|anchor| self.resolve_anchor(&anchor))
                                .transpose()
                        })
                        .collect::<Result<Vec<_>, ReadError>>()?,
                );
            }
            resolved.push((glyph, name, rows));
        }

        let class_anchors =
            Self::build_class_anchor_map(resolved.iter().flat_map(|(_, name, rows)| {
                rows.iter().map(|anchors| (name.clone(), anchors.clone()))
            }));
        // a ligature's anchors are named per component, as `top_1`, `top_2`
        let ligature = !null_anchors_when_empty;
        let mut ligature_components: IndexMap<u16, Vec<(SmolStr, usize, FeaAnchor)>> =
            IndexMap::new();
        if ligature {
            for (_, name, rows) in &resolved {
                for (component, anchors) in rows.iter().enumerate() {
                    for (class, anchor) in anchors.iter().enumerate() {
                        if let Some(anchor) = anchor {
                            ligature_components.entry(class as u16).or_default().push((
                                name.clone(),
                                component + 1,
                                anchor.clone(),
                            ));
                        }
                    }
                }
            }
        }
        self.ligature_components = ligature_components;
        let class_names = self.prepare_guessed_mark_classes(mark_classes, &class_anchors, ligature);

        let mut statements = vec![];
        for (glyph, name, rows) in resolved {
            let mut components = vec![];
            for (component, anchors) in rows.into_iter().enumerate() {
                let empty = anchors.iter().all(Option::is_none);
                components.push(self.materialize_anchor_mark_classes(
                    &name,
                    &anchors,
                    &class_names,
                    ligature.then_some(component + 1),
                    |class| {
                        (null_anchors_when_empty && empty) || !class_anchors.contains_key(&class)
                    },
                ));
            }
            statements.push((glyph, components));
        }
        Ok(statements)
    }

    fn prepare_guessed_mark_classes(
        &mut self,
        mark_classes: IndexMap<u16, Vec<(GlyphContainer, FeaAnchor)>>,
        mark_class_to_target_glyph_anchor: &IndexMap<u16, IndexMap<SmolStr, FeaAnchor>>,
        ligature: bool,
    ) -> IndexMap<u16, SmolStr> {
        let guessed_names =
            self.guess_anchor_names(&mark_classes, mark_class_to_target_glyph_anchor, ligature);
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
                .unwrap_or_else(|| self.mark_class_symbol(class_number));
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
    /// `component` is the 1-based ligature component the anchors belong to: a
    /// ligature's anchor is named for its component, as `top_1`, `top_2`.
    fn materialize_anchor_mark_classes(
        &mut self,
        target_glyph_name: &SmolStr,
        class_anchors: &[Option<FeaAnchor>],
        class_to_anchor_name: &IndexMap<u16, SmolStr>,
        component: Option<usize>,
        keep_null: impl Fn(u16) -> bool,
    ) -> Vec<(Option<FeaAnchor>, MarkClass)> {
        let mut anchors_mark_classes = vec![];
        for (class_number, anchor) in class_anchors.iter().enumerate() {
            let Some(anchor_name) = class_to_anchor_name.get(&(class_number as u16)) else {
                continue;
            };
            let registered_name = match component {
                Some(component) => SmolStr::new(format!("{anchor_name}_{component}")),
                None => anchor_name.clone(),
            };
            match anchor {
                Some(anchor) => {
                    self.register_anchor(target_glyph_name, anchor, Some(&registered_name))
                }
                None if !keep_null(class_number as u16) => continue,
                None => {}
            }
            anchors_mark_classes.push((anchor.clone(), MarkClass::new(anchor_name)));
        }
        anchors_mark_classes
    }

    /// Names each mark class of a lookup, as its anchors are named on glyphs.
    ///
    /// A class of the same marks with the same anchors as a class an earlier
    /// lookup named (as a mark-to-mark lookup's often is) takes that name. An
    /// ogonek class is `ogonek`. Otherwise a class is named for where most of
    /// its anchors sit on their glyphs, if that name is free, else for its
    /// number.
    fn guess_anchor_names(
        &mut self,
        mark_classes: &IndexMap<u16, Vec<(GlyphContainer, FeaAnchor)>>,
        mark_class_to_base_glyph_anchor: &IndexMap<u16, IndexMap<SmolStr, FeaAnchor>>,
        ligature: bool,
    ) -> Vec<SmolStr> {
        let mut new_names = vec![];
        for (class_number, base_glyphs_anchors) in mark_class_to_base_glyph_anchor {
            let marks = mark_classes.get(class_number);
            if let Some(name) = marks.and_then(|marks| self.same_mark_class(marks)) {
                // A ligature's anchors are named per component
                let agree = if ligature {
                    self.ligature_anchors_agree(&name, *class_number)
                } else {
                    self.anchors_agree(&name, base_glyphs_anchors)
                };
                if agree {
                    if !ligature {
                        self.anchors
                            .entry(name.clone())
                            .or_default()
                            .extend(base_glyphs_anchors.clone());
                    }
                    new_names.push(name);
                    continue;
                }
            }
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
            let zone = majority_in_quadrant(&xs, &ys);
            let ogonek = marks.is_some_and(|marks| {
                !marks.is_empty()
                    && marks.iter().all(|(glyphs, _)| match glyphs {
                        GlyphContainer::GlyphName(name) => self.ogonek_glyphs.contains(&name.name),
                        _ => false,
                    })
            }) && matches!(zone, Some("bottom" | "bottomright") | None);
            let name = if ogonek { Some("ogonek") } else { zone };
            if let Some(name) = name
                && self.anchors.get(name).is_none()
                && self.mark_classes.get(name).is_none()
            {
                new_names.push(name.into());
                if !ligature {
                    self.anchors
                        .insert(name.into(), base_glyphs_anchors.clone());
                }
                continue;
            }
            // Create one with a symbol
            let name = self.mark_class_symbol(*class_number);
            if !ligature {
                self.anchors
                    .insert(name.clone(), base_glyphs_anchors.clone());
            }
            new_names.push(name);
        }
        new_names
    }

    /// The name of a mark class already defined with exactly these marks and anchors
    fn same_mark_class(&self, marks: &[(GlyphContainer, FeaAnchor)]) -> Option<SmolStr> {
        let mut wanted: Vec<(String, String)> = marks
            .iter()
            .map(|(glyphs, anchor)| (glyphs.as_fea(""), anchor.as_fea("")))
            .collect();
        wanted.sort();
        self.mark_classes.iter().find_map(|(name, definitions)| {
            let mut defined: Vec<(String, String)> = definitions
                .iter()
                .map(|d| (d.glyphs.as_fea(""), d.anchor.as_fea("")))
                .collect();
            defined.sort();
            (defined == wanted).then(|| name.clone())
        })
    }

    /// A class of a pair positioning class definition: by name where it is
    /// large, so that every pair it is in does not write it out again (class
    /// kerning of hundreds of classes otherwise writes gigabytes)
    fn pair_class(&mut self, glyphs: Option<&Vec<GlyphContainer>>) -> GlyphContainer {
        let glyphs = glyphs.cloned().unwrap_or_default();
        if glyphs.len() < crate::PROMOTE_TO_NAMED_CLASS_THRESHOLD {
            return GlyphContainer::GlyphClass(GlyphClass::new(glyphs, 0..0));
        }
        let key = glyphs
            .iter()
            .map(|g| g.as_fea(""))
            .collect::<Vec<_>>()
            .join(" ");
        if let Some(name) = self.pair_class_names.get(&key) {
            return GlyphContainer::GlyphClassName(format!("@{name}").into());
        }
        let name: SmolStr = format!(
            "{}{}",
            crate::PAIR_CLASS_PREFIX,
            self.pair_class_names.len() + 1
        )
        .into();
        self.pair_class_names.insert(key, name.clone());
        self.named_classes
            .insert(name.clone(), GlyphClass::new(glyphs, 0..0));
        GlyphContainer::GlyphClassName(format!("@{name}").into())
    }

    /// Whether no ligature has an anchor of this name and its component
    /// somewhere else already, for the anchors of a mark class
    fn ligature_anchors_agree(&self, name: &str, class_number: u16) -> bool {
        self.ligature_components
            .get(&class_number)
            .into_iter()
            .flatten()
            .all(|(glyph, component, anchor)| {
                self.anchors
                    .get(format!("{name}_{component}").as_str())
                    .and_then(|existing| existing.get(glyph))
                    .is_none_or(|known| known.as_fea("") == anchor.as_fea(""))
            })
    }

    /// Whether no glyph has an anchor of this name somewhere else already
    fn anchors_agree(&self, name: &str, anchors: &IndexMap<SmolStr, FeaAnchor>) -> bool {
        let Some(existing) = self.anchors.get(name) else {
            return true;
        };
        anchors.iter().all(|(glyph, anchor)| {
            existing
                .get(glyph)
                .is_none_or(|known| known.as_fea("") == anchor.as_fea(""))
        })
    }
}

/// GPOS lookup type 1.
struct SinglePosKind;

impl<'a> LookupKind<'a> for SinglePosKind {
    type Subtable = SinglePos<'a>;
    type Extension = ExtensionPosFormat1<'a, SinglePos<'a>>;
    const PREFIX: &'static str = "gpos_single";

    fn uncompile_subtable(
        ctx: &mut UncompileContext<'a>,
        block: &mut LookupBlock,
        subtable: Self::Subtable,
    ) -> Result<(), ReadError> {
        ctx.uncompile_single_pos(block, &subtable)
    }
}

/// GPOS lookup type 2.
struct PairPosKind;

impl<'a> LookupKind<'a> for PairPosKind {
    type Subtable = PairPos<'a>;
    type Extension = ExtensionPosFormat1<'a, PairPos<'a>>;
    const PREFIX: &'static str = "gpos_pair";

    fn uncompile_subtable(
        ctx: &mut UncompileContext<'a>,
        block: &mut LookupBlock,
        subtable: Self::Subtable,
    ) -> Result<(), ReadError> {
        ctx.uncompile_pair_pos(block, &subtable)
    }
}

/// GPOS lookup type 3.
struct CursivePosKind;

impl<'a> LookupKind<'a> for CursivePosKind {
    type Subtable = CursivePos<'a>;
    type Extension = ExtensionPosFormat1<'a, CursivePos<'a>>;
    const PREFIX: &'static str = "gpos_cursive";

    fn uncompile_subtable(
        ctx: &mut UncompileContext<'a>,
        block: &mut LookupBlock,
        subtable: Self::Subtable,
    ) -> Result<(), ReadError> {
        ctx.uncompile_cursive_pos(block, &subtable)
    }
}

/// GPOS lookup type 4.
struct MarkBasePosKind;

impl<'a> LookupKind<'a> for MarkBasePosKind {
    type Subtable = MarkBasePos<'a>;
    type Extension = ExtensionPosFormat1<'a, MarkBasePos<'a>>;
    const PREFIX: &'static str = "gpos_mark_to_base";

    fn uncompile_subtable(
        ctx: &mut UncompileContext<'a>,
        block: &mut LookupBlock,
        subtable: Self::Subtable,
    ) -> Result<(), ReadError> {
        ctx.uncompile_mark_base_pos(block, &subtable)
    }
}

/// GPOS lookup type 5.
struct MarkLigPosKind;

impl<'a> LookupKind<'a> for MarkLigPosKind {
    type Subtable = MarkLigPos<'a>;
    type Extension = ExtensionPosFormat1<'a, MarkLigPos<'a>>;
    const PREFIX: &'static str = "gpos_mark_to_ligature";

    fn uncompile_subtable(
        ctx: &mut UncompileContext<'a>,
        block: &mut LookupBlock,
        subtable: Self::Subtable,
    ) -> Result<(), ReadError> {
        ctx.uncompile_mark_lig_pos(block, &subtable)
    }
}

/// GPOS lookup type 6.
struct MarkMarkPosKind;

impl<'a> LookupKind<'a> for MarkMarkPosKind {
    type Subtable = MarkMarkPos<'a>;
    type Extension = ExtensionPosFormat1<'a, MarkMarkPos<'a>>;
    const PREFIX: &'static str = "gpos_mark_to_mark";

    fn uncompile_subtable(
        ctx: &mut UncompileContext<'a>,
        block: &mut LookupBlock,
        subtable: Self::Subtable,
    ) -> Result<(), ReadError> {
        ctx.uncompile_mark_mark_pos(block, &subtable)
    }
}

/// GPOS lookup type 7.
struct ContextualKind;

impl<'a> LookupKind<'a> for ContextualKind {
    type Subtable = PositionSequenceContext<'a>;
    type Extension = ExtensionPosFormat1<'a, PositionSequenceContext<'a>>;
    const PREFIX: &'static str = "gpos_contextual";

    fn uncompile_subtable(
        ctx: &mut UncompileContext<'a>,
        block: &mut LookupBlock,
        subtable: Self::Subtable,
    ) -> Result<(), ReadError> {
        block.statements.extend(
            ctx.uncompile_sequence_context(subtable, Pos)?
                .into_iter()
                .map(to_context_statement),
        );
        Ok(())
    }
}

/// GPOS lookup type 8.
struct ChainContextualKind;

impl<'a> LookupKind<'a> for ChainContextualKind {
    type Subtable = PositionChainContext<'a>;
    type Extension = ExtensionPosFormat1<'a, PositionChainContext<'a>>;
    const PREFIX: &'static str = "gpos_chain_contextual";

    fn uncompile_subtable(
        ctx: &mut UncompileContext<'a>,
        block: &mut LookupBlock,
        subtable: Self::Subtable,
    ) -> Result<(), ReadError> {
        block.statements.extend(
            ctx.uncompile_chain_sequence_context(subtable, Pos)?
                .into_iter()
                .map(to_context_statement),
        );
        Ok(())
    }
}

impl_uncompile_subtables!(PositionSubtables, empty = "gpos_extension", {
    Single => SinglePosKind,
    Pair => PairPosKind,
    Cursive => CursivePosKind,
    MarkToBase => MarkBasePosKind,
    MarkToLig => MarkLigPosKind,
    MarkToMark => MarkMarkPosKind,
    Contextual => ContextualKind,
    ChainContextual => ChainContextualKind,
});

/// Whether a value record moves anything, in its values or its devices
fn has_value(record: &ValueRecord) -> bool {
    [
        record.x_placement(),
        record.y_placement(),
        record.x_advance(),
        record.y_advance(),
    ]
    .iter()
    .any(|value| value.is_some_and(|value| value != 0))
        || [
            ValueFormat::X_PLACEMENT_DEVICE,
            ValueFormat::Y_PLACEMENT_DEVICE,
            ValueFormat::X_ADVANCE_DEVICE,
            ValueFormat::Y_ADVANCE_DEVICE,
        ]
        .into_iter()
        .any(|field| {
            record
                .device_offset(field)
                .is_some_and(|offset| !offset.is_null())
        })
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
