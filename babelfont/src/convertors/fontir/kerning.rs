use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use fontdrasil::{
    coords::NormalizedLocation,
    orchestration::{Access, AccessBuilder, Work},
    types::GlyphName,
};
use fontir::{
    error::{BadSourceKind, Error},
    ir::{GlyphOrder, KernGroup, KernSide, KerningInstance, KerningLocations},
    orchestration::{Context, WorkId},
};
use ordered_float::OrderedFloat;
use smol_str::SmolStr;

use crate::Font;

#[derive(Debug)]
pub(crate) struct KerningLocationsWork(pub Arc<Font>);

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

impl Work<Context, WorkId, Error> for KerningLocationsWork {
    fn id(&self) -> WorkId {
        WorkId::KerningLocations
    }

    fn read_access(&self) -> Access<WorkId> {
        Access::None
    }

    fn exec(&self, context: &Context) -> Result<(), fontir::error::Error> {
        log::trace!("Generate IR kerning locations");
        let font = &self.0;
        let axes = font.fontdrasil_axes().map_err(|e| {
            Error::BadSource(fontir::error::BadSource::new(
                self.0.source.clone().unwrap_or("unknown source".into()),
                BadSourceKind::Custom(format!("Error converting axes for kerning: {e}")),
            ))
        })?;

        let default_master_id = font
            .default_master()
            .map(|m| m.id.as_str())
            .or_else(|| font.masters.first().map(|m| m.id.as_str()));

        let mut kerning_locations = KerningLocations::default();
        for master in &font.masters {
            let keep = default_master_id == Some(master.id.as_str())
                || !font.merged_kerning_for_master(master).is_empty();
            if keep {
                kerning_locations.locations.insert(
                    master
                        .location
                        .to_normalized(&axes)
                        .map_err(fontir::error::Error::CoordinateConversionError)?,
                );
            }
        }

        context.kerning_locations.set(kerning_locations);
        Ok(())
    }
}

fn derive_kern_groups(font: &Font) -> BTreeMap<KernGroup, BTreeSet<GlyphName>> {
    let (first, second) = font.kern_groups_with_rtl_swaps();
    let mut groups: BTreeMap<KernGroup, BTreeSet<GlyphName>> = BTreeMap::new();

    for (group, members) in &first {
        groups.insert(
            KernGroup::Side1(group.clone()),
            members.iter().map(GlyphName::new).collect(),
        );
    }
    for (group, members) in &second {
        groups.insert(
            KernGroup::Side2(group.clone()),
            members.iter().map(GlyphName::new).collect(),
        );
    }

    groups
}

impl Work<Context, WorkId, Error> for KerningInstanceWork {
    fn id(&self) -> WorkId {
        WorkId::KernInstance(self.location.clone())
    }

    fn read_access(&self) -> Access<WorkId> {
        AccessBuilder::new().variant(WorkId::GlyphOrder).build()
    }

    fn exec(&self, context: &Context) -> Result<(), Error> {
        log::trace!("Generate IR for kerning at {:?}", self.location);
        let groups = derive_kern_groups(&self.font);
        let arc_glyph_order = context.glyph_order.get();
        let glyph_order = arc_glyph_order.as_ref();

        let mut kerning = KerningInstance {
            location: self.location.clone(),
            ..Default::default()
        };

        // let bracket_glyph_map = make_bracket_glyph_map(glyph_order);

        if let Some(kern_pairs) = kerning_at_location(&self.font, &self.location) {
            kern_pairs
                .iter()
                .filter_map(|((side1, side2), pos_adjust)| {
                    let side1 = kern_participant(glyph_order, &groups, side1, true);
                    let side2 = kern_participant(glyph_order, &groups, side2, false);
                    side1.zip(side2).map(|side| (side, *pos_adjust))
                })
                // .flat_map(|(participants, value)| {
                //     expand_kerning_to_brackets(&bracket_glyph_map, participants, value)
                // })
                .for_each(|(participants, value)| {
                    *kerning.kerns.entry(participants).or_default() = value;
                });
        }

        kerning.groups = groups;

        context.kerning_at.set(kerning);
        Ok(())
    }
}

type Kerns = BTreeMap<(SmolStr, SmolStr), OrderedFloat<f64>>;

/// Get the merged LTR+RTL kerning pairs for a given master at a location.
///
/// Uses `crate::kerning::merge_kerning` to produce a single flat set of pairs
/// from both `master.kerning` and `format_specific["...kerningRTL"]`.
fn kerning_at_location(font: &Font, location: &NormalizedLocation) -> Option<Kerns> {
    let axes = font.fontdrasil_axes().ok()?;
    let master = font.masters.iter().find(|master| {
        master
            .location
            .to_normalized(&axes)
            .is_ok_and(|normalized| normalized == *location)
    })?;

    let merged = font.merged_kerning_for_master(master);

    Some(
        merged
            .into_iter()
            .map(|((l, r), v)| ((l, r), OrderedFloat(v as f64)))
            .collect(),
    )
}
