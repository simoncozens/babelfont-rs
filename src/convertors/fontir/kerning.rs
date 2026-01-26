use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use fontdrasil::{
    coords::NormalizedLocation,
    orchestration::{Access, AccessBuilder, Work},
    types::GlyphName,
};
use fontir::{
    error::Error,
    ir::{GlyphOrder, KernGroup, KernSide, KerningGroups, KerningInstance},
    orchestration::{Context, WorkId},
};
use ordered_float::OrderedFloat;
use smol_str::SmolStr;

use crate::Font;

#[derive(Debug)]
pub(crate) struct KerningGroupWork(pub Arc<Font>);

#[derive(Debug)]
pub(crate) struct KerningInstanceWork {
    pub font: Arc<Font>,
    pub location: NormalizedLocation,
}

/// See <https://github.com/googlefonts/glyphsLib/blob/42bc1db912fd4b66f130fb3bdc63a0c1e774eb38/Lib/glyphsLib/builder/kerning.py#L53-L72>
fn kern_participant(
    glyph_order: &GlyphOrder,
    groups: &BTreeMap<KernGroup, BTreeSet<GlyphName>>,
    raw_side: &str,
    first: bool,
) -> Option<KernSide> {
    if let Some(group) = raw_side.strip_prefix('@') {
        let key = if first {
            KernGroup::Side1(group.into())
        } else {
            KernGroup::Side2(group.into())
        };
        if groups.contains_key(&key) {
            Some(KernSide::Group(key))
        } else {
            log::warn!("Invalid kern side: {raw_side}, no group {group:?}");
            None
        }
    } else {
        let name = GlyphName::from(raw_side);
        if glyph_order.contains(&name) {
            Some(KernSide::Glyph(name))
        } else {
            log::warn!("Invalid kern side: {raw_side}, no such glyph");
            None
        }
    }
}

impl Work<Context, WorkId, Error> for KerningGroupWork {
    fn id(&self) -> WorkId {
        WorkId::KerningGroups
    }

    fn read_access(&self) -> Access<WorkId> {
        Access::None
    }

    fn exec(&self, context: &Context) -> Result<(), fontir::error::Error> {
        log::trace!("Generate IR for kerning groups");
        let font = &self.0;
        let axes = font.fontdrasil_axes()?;

        let mut groups = KerningGroups::default();

        for (group, members) in font.first_kern_groups.iter() {
            groups.groups.insert(
                KernGroup::Side1(group.clone()),
                members.iter().map(GlyphName::new).collect(),
            );
        }
        for (group, members) in font.second_kern_groups.iter() {
            groups.groups.insert(
                KernGroup::Side2(group.clone()),
                members.iter().map(GlyphName::new).collect(),
            );
        }
        let mut normalized_locations = BTreeSet::new();
        for master in &font.masters {
            normalized_locations.insert(
                master
                    .location
                    .to_normalized(&axes)
                    .map_err(fontir::error::Error::CoordinateConversionError)?,
            );
        }

        groups.locations = normalized_locations;

        context.kerning_groups.set(groups);
        Ok(())
    }
}

impl Work<Context, WorkId, Error> for KerningInstanceWork {
    fn id(&self) -> WorkId {
        WorkId::KernInstance(self.location.clone())
    }

    fn read_access(&self) -> Access<WorkId> {
        AccessBuilder::new()
            .variant(WorkId::GlyphOrder)
            .variant(WorkId::KerningGroups)
            .build()
    }

    fn exec(&self, context: &Context) -> Result<(), Error> {
        log::trace!("Generate IR for kerning at {:?}", self.location);
        let kerning_groups = context.kerning_groups.get();
        let groups = &kerning_groups.groups;
        let arc_glyph_order = context.glyph_order.get();
        let glyph_order = arc_glyph_order.as_ref();

        let mut kerning = KerningInstance {
            location: self.location.clone(),
            ..Default::default()
        };

        // let bracket_glyph_map = make_bracket_glyph_map(glyph_order);

        let Some(kern_pairs) = kerning_at_location(&self.font, &self.location) else {
            return Ok(());
        };

        kern_pairs
            .iter()
            .filter_map(|((side1, side2), pos_adjust)| {
                let side1 = kern_participant(glyph_order, groups, side1, true);
                let side2 = kern_participant(glyph_order, groups, side2, false);
                side1.zip(side2).map(|side| (side, *pos_adjust))
            })
            // .flat_map(|(participants, value)| {
            //     expand_kerning_to_brackets(&bracket_glyph_map, participants, value)
            // })
            .for_each(|(participants, value)| {
                *kerning.kerns.entry(participants).or_default() = value;
            });

        context.kerning_at.set(kerning);
        Ok(())
    }
}

type Kerns = BTreeMap<(SmolStr, SmolStr), OrderedFloat<f64>>;

/// get the combined LTR & RTL kerns at the given location.
///
/// If only LTR exists, it can be borrowed directly. If RTL exists, it has to
/// be converted into LTR.
///
/// see <https://github.com/googlefonts/glyphsLib/blob/682ff4b17711/Lib/glyphsLib/builder/kerning.py#L41>
fn kerning_at_location<'a>(
    font: &'a Font,
    location: &NormalizedLocation,
) -> Option<Cow<'a, Kerns>> {
    let axes = font.fontdrasil_axes().ok()?;
    let master = font.masters.iter().find(|master| {
        master
            .location
            .to_normalized(&axes)
            .is_ok_and(|normalized| normalized == *location)
    })?;
    Some(Cow::Owned(
        master
            .kerning
            .iter()
            .map(|((side1, side2), value)| {
                ((side1.clone(), side2.clone()), OrderedFloat(*value as f64))
            })
            .collect::<Kerns>(),
    ))
}
