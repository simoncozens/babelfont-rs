//! Format-agnostic adapters over the layout subtable types in `read-fonts`.
//!
//! Every OpenType lookup type comes in two widths: the original formats, with
//! 16-bit glyph IDs and offsets, and the "wide" formats, with 24-bit glyph IDs
//! and 32-bit offsets. `read-fonts` models these as separate types (and
//! separate variants of the subtable enums) which share accessor names but not
//! types, so un-compiling each of them separately means writing the same code
//! twice for every lookup type - and four times for the lookup types which
//! also come in two shapes.
//!
//! Instead we normalise them here:
//!
//! * one small *source* trait per lookup type, implemented once for each
//!   shape and shared by both widths, which reads a subtable's contents out as
//!   plain Rust data (`GlyphId`s, `ValueRecord`s, ...) with every format
//!   difference already resolved;
//! * [`LookupKind`], which describes a whole lookup type - its subtable type,
//!   the prefix used to name the lookups we generate, and how to un-compile a
//!   single subtable - so that
//!   [`UncompileContext::uncompile_lookup_subtables`] can walk any lookup
//!   without knowing its type;
//! * [`UncompileSubtables`], which turns everything a lookup holds into a
//!   populated lookup block.
//!
//! Adding a new subtable format then means adding one implementation of the
//! relevant source trait; nothing in `gsub.rs` or `gpos.rs` needs to change.

use fea_rs_ast::{LookupBlock, SubOrPos};
use skrifa::{
    GlyphId,
    raw::{
        FontRead, ReadError,
        tables::{
            gpos::{
                AnchorTable, CursivePos, CursivePosFormat1, CursivePosFormat2, MarkBasePos,
                MarkBasePosFormat1, MarkBasePosFormat2, MarkLigPos, MarkLigPosFormat1,
                MarkLigPosFormat2, MarkMarkPos, MarkMarkPosFormat1, MarkMarkPosFormat2, PairPos,
                PairPosFormat1, PairPosFormat2, PairPosFormat3, PairPosFormat4, SinglePos,
                SinglePosFormat1, SinglePosFormat2, SinglePosFormat3, SinglePosFormat4,
                ValueRecord,
            },
            gsub::{
                AlternateSubst, AlternateSubstFormat1, AlternateSubstFormat2, LigatureSubst,
                LigatureSubstFormat1, LigatureSubstFormat2, MultipleSubst, MultipleSubstFormat1,
                MultipleSubstFormat2, ReverseChainSingleSubst, ReverseChainSingleSubstFormat1,
                ReverseChainSingleSubstFormat2, SingleSubst, SingleSubstFormat1,
                SingleSubstFormat2, SingleSubstFormat3, SingleSubstFormat4,
            },
            layout::{
                ChainedSequenceContext, ChainedSequenceContextFormat1,
                ChainedSequenceContextFormat2, ChainedSequenceContextFormat3,
                ChainedSequenceContextFormat4, ChainedSequenceContextFormat5, ClassDef,
                CoverageTable, ExtensionLookup, SequenceContext, SequenceContextFormat1,
                SequenceContextFormat2, SequenceContextFormat3, SequenceContextFormat4,
                SequenceContextFormat5, SequenceContextFormat6, SequenceLookupRecord, Subtables,
            },
        },
        types::{BigEndian, Scalar},
    },
};

use crate::{UncompileContext, add_subtable_break};

/// A single lookup type: enough for the driver to walk a lookup's subtables.
///
/// Implemented by the marker types in `gsub.rs` and `gpos.rs`, one per
/// OpenType lookup type.
pub(crate) trait LookupKind<'a> {
    /// The enum of subtables for this lookup type.
    type Subtable: FontRead<'a, Args = ()> + 'a;
    /// The extension subtable wrapper for this lookup type.
    type Extension: ExtensionLookup<'a, Self::Subtable> + 'a;
    /// The prefix used to name the lookup blocks we generate.
    const PREFIX: &'static str;
    /// Append everything `subtable` describes to `block`.
    fn uncompile_subtable(
        ctx: &mut UncompileContext<'a>,
        block: &mut LookupBlock,
        subtable: Self::Subtable,
    ) -> Result<(), ReadError>;
}

/// The subtables of a lookup, with the width of their offsets abstracted away.
pub(crate) trait UncompileSubtables<'a>: Sized {
    /// The prefix used to name the lookup blocks we generate.
    fn prefix(&self) -> &'static str;
    /// Un-compile every subtable into a fresh lookup block.
    fn uncompile<P: SubOrPos>(
        self,
        ctx: &mut UncompileContext<'a>,
        index: u16,
        phase: P,
    ) -> Result<LookupBlock, ReadError>;
}

impl<'a> UncompileContext<'a> {
    /// Un-compile all the subtables of one lookup into a lookup block.
    ///
    /// Each subtable is preceded by a `subtable` break, and the caller gets an
    /// empty block back if the lookup has no subtables at all (as happens with
    /// an extension lookup).
    pub(crate) fn uncompile_lookup_subtables<K: LookupKind<'a>, P: SubOrPos>(
        &mut self,
        index: u16,
        phase: P,
        subtables: Subtables<'a, K::Subtable, K::Extension>,
    ) -> Result<LookupBlock, ReadError> {
        let mut block = self.create_lookup_block(index, phase);
        for subtable in subtables.iter().flatten() {
            add_subtable_break(&mut block);
            K::uncompile_subtable(self, &mut block, subtable)?;
        }
        Ok(block)
    }
}

/// Implement [`UncompileSubtables`] for a GSUB or GPOS subtable collection.
///
/// The list of `Variant => Kind` pairs is the single source of truth for both
/// the naming prefix and the way a subtable is un-compiled, so supporting a
/// new lookup type is a one-line addition.
macro_rules! impl_uncompile_subtables {
    ($enum:ident, empty = $empty:literal, { $($variant:ident => $kind:ty),* $(,)? }) => {
        impl<'a> $crate::layout::UncompileSubtables<'a> for $enum<'a> {
            fn prefix(&self) -> &'static str {
                match self {
                    $( Self::$variant(_) => <$kind as $crate::layout::LookupKind<'a>>::PREFIX, )*
                    Self::EmptyExtension => $empty,
                }
            }

            fn uncompile<P: fea_rs_ast::SubOrPos>(
                self,
                ctx: &mut $crate::UncompileContext<'a>,
                index: u16,
                phase: P,
            ) -> Result<fea_rs_ast::LookupBlock, $crate::skrifa::raw::ReadError> {
                match self {
                    $(
                        Self::$variant(subtables) =>
                            ctx.uncompile_lookup_subtables::<$kind, P>(index, phase, subtables),
                    )*
                    Self::EmptyExtension => Ok(ctx.create_lookup_block(index, phase)),
                }
            }
        }
    };
}
pub(crate) use impl_uncompile_subtables;

/// Implement a normalising trait for the enum of all the formats of a
/// subtable, by asking whichever format we actually have.
///
/// This relies on every method having the same normalised signature on every
/// format, and calls them through the trait rather than on the table, because
/// some tables have inherent methods of the same name with a different type.
macro_rules! delegate_to_formats {
    (
        impl<'a> $trait:ident for $enum:ident {
            $( fn $name:ident(&self) -> $ret:ty; )*
        }
        [$($format:ident),* $(,)?]
    ) => {
        impl<'a> $trait<'a> for $enum<'a> {
            delegate_to_formats!(@methods [$($format),*] $trait; $( fn $name(&self) -> $ret; )*);
        }
    };
    (@methods [$($format:ident),*] $trait:ident; ) => {};
    (@methods [$($format:ident),*] $trait:ident; fn $name:ident(&self) -> $ret:ty; $($rest:tt)*) => {
        fn $name(&self) -> $ret {
            match self {
                $( Self::$format(format) => $trait::$name(format), )*
            }
        }
        delegate_to_formats!(@methods [$($format),*] $trait; $($rest)*);
    };
}

/// Implement a normalising trait identically for several table types.
///
/// The narrow and wide formats of one table expose the same accessors and
/// differ only in the width of what they return, so one body does for both.
macro_rules! impl_for_widths {
    (
        impl<'a> $trait:ident for [$($table:ident),+ $(,)?] $body:tt
    ) => {
        $(
            impl<'a> $trait<'a> for $table<'a> $body
        )+
    };
}

fn glyph_ids<T: Scalar + Into<GlyphId>>(ids: &[BigEndian<T>]) -> Vec<GlyphId> {
    ids.iter().map(|id| id.get().into()).collect()
}

fn class_ids(ids: &[BigEndian<u16>]) -> Vec<u32> {
    ids.iter().map(|id| id.get() as u32).collect()
}

fn lookup_records(records: &[SequenceLookupRecord]) -> Vec<(u16, u16)> {
    records
        .iter()
        .map(|record| (record.sequence_index(), record.lookup_list_index()))
        .collect()
}

/// Apply a delta to every glyph of a coverage.
///
/// Glyph IDs wrap within the width of the subtable's own glyph IDs: the
/// original formats are 16-bit and the wide formats are 24-bit.
fn delta_substitutes(coverage: &CoverageTable<'_>, delta: i32, max_glyph_id: u32) -> Vec<GlyphId> {
    coverage
        .iter()
        .map(|glyph| GlyphId::new(glyph.to_u32().wrapping_add_signed(delta) & max_glyph_id))
        .collect()
}

pub(crate) trait SingleSubstSource<'a> {
    fn coverage(&self) -> Result<CoverageTable<'a>, ReadError>;
    /// The replacement glyph for each covered glyph.
    fn substitutes(&self) -> Result<Vec<GlyphId>, ReadError>;
}

impl<'a> SingleSubstSource<'a> for SingleSubstFormat1<'a> {
    fn coverage(&self) -> Result<CoverageTable<'a>, ReadError> {
        self.coverage()
    }

    fn substitutes(&self) -> Result<Vec<GlyphId>, ReadError> {
        Ok(delta_substitutes(
            &self.coverage()?,
            self.delta_glyph_id().into(),
            u16::MAX as u32,
        ))
    }
}

impl<'a> SingleSubstSource<'a> for SingleSubstFormat3<'a> {
    fn coverage(&self) -> Result<CoverageTable<'a>, ReadError> {
        self.coverage()
    }

    fn substitutes(&self) -> Result<Vec<GlyphId>, ReadError> {
        Ok(delta_substitutes(
            &self.coverage()?,
            self.delta_glyph_id().into(),
            0xFF_FFFF,
        ))
    }
}

impl_for_widths! {
    impl<'a> SingleSubstSource for [SingleSubstFormat2, SingleSubstFormat4] {
        fn coverage(&self) -> Result<CoverageTable<'a>, ReadError> {
            self.coverage()
        }

        fn substitutes(&self) -> Result<Vec<GlyphId>, ReadError> {
            Ok(glyph_ids(self.substitute_glyph_ids()))
        }
    }
}

delegate_to_formats! {
    impl<'a> SingleSubstSource for SingleSubst {
        fn coverage(&self) -> Result<CoverageTable<'a>, ReadError>;
        fn substitutes(&self) -> Result<Vec<GlyphId>, ReadError>;
    }
    [Format1, Format2, Format3, Format4]
}

pub(crate) trait MultipleSubstSource<'a> {
    /// One replacement sequence per covered glyph.
    fn sequences(&self) -> Vec<Vec<GlyphId>>;
}

impl_for_widths! {
    impl<'a> MultipleSubstSource for [MultipleSubstFormat1, MultipleSubstFormat2] {
        fn sequences(&self) -> Vec<Vec<GlyphId>> {
            self.sequences()
                .iter()
                .flatten()
                .map(|sequence| glyph_ids(sequence.substitute_glyph_ids()))
                .collect()
        }
    }
}

delegate_to_formats! {
    impl<'a> MultipleSubstSource for MultipleSubst {
        fn sequences(&self) -> Vec<Vec<GlyphId>>;
    }
    [Format1, Format2]
}

pub(crate) trait AlternateSubstSource<'a> {
    fn coverage(&self) -> Result<CoverageTable<'a>, ReadError>;
    /// One set of alternates per covered glyph.
    fn alternate_sets(&self) -> Vec<Vec<GlyphId>>;
}

impl_for_widths! {
    impl<'a> AlternateSubstSource for [AlternateSubstFormat1, AlternateSubstFormat2] {
        fn coverage(&self) -> Result<CoverageTable<'a>, ReadError> {
            self.coverage()
        }

        fn alternate_sets(&self) -> Vec<Vec<GlyphId>> {
            self.alternate_sets()
                .iter()
                .flatten()
                .map(|set| glyph_ids(set.alternate_glyph_ids()))
                .collect()
        }
    }
}

delegate_to_formats! {
    impl<'a> AlternateSubstSource for AlternateSubst {
        fn coverage(&self) -> Result<CoverageTable<'a>, ReadError>;
        fn alternate_sets(&self) -> Vec<Vec<GlyphId>>;
    }
    [Format1, Format2]
}

/// The glyph a ligature produces and the components that make it up.
pub(crate) struct Ligature {
    pub glyph: GlyphId,
    pub components: Vec<GlyphId>,
}

pub(crate) trait LigatureSubstSource<'a> {
    fn coverage(&self) -> Result<CoverageTable<'a>, ReadError>;
    /// One set of ligatures per covered glyph.
    fn ligature_sets(&self) -> Vec<Vec<Ligature>>;
}

impl_for_widths! {
    impl<'a> LigatureSubstSource for [LigatureSubstFormat1, LigatureSubstFormat2] {
        fn coverage(&self) -> Result<CoverageTable<'a>, ReadError> {
            self.coverage()
        }

        fn ligature_sets(&self) -> Vec<Vec<Ligature>> {
            self.ligature_sets()
                .iter()
                .flatten()
                .map(|set| {
                    set.ligatures()
                        .iter()
                        .flatten()
                        .map(|ligature| Ligature {
                            glyph: ligature.ligature_glyph().into(),
                            components: glyph_ids(ligature.component_glyph_ids()),
                        })
                        .collect()
                })
                .collect()
        }
    }
}

delegate_to_formats! {
    impl<'a> LigatureSubstSource for LigatureSubst {
        fn coverage(&self) -> Result<CoverageTable<'a>, ReadError>;
        fn ligature_sets(&self) -> Vec<Vec<Ligature>>;
    }
    [Format1, Format2]
}

pub(crate) trait ReverseChainSubstSource<'a> {
    fn coverage(&self) -> Result<CoverageTable<'a>, ReadError>;
    /// The replacement for each covered glyph.
    fn substitutes(&self) -> Vec<GlyphId>;
    /// The backtrack coverages, closest first.
    fn backtrack(&self) -> Vec<CoverageTable<'a>>;
    fn lookahead(&self) -> Vec<CoverageTable<'a>>;
}

impl_for_widths! {
    impl<'a> ReverseChainSubstSource for
        [ReverseChainSingleSubstFormat1, ReverseChainSingleSubstFormat2]
    {
        fn coverage(&self) -> Result<CoverageTable<'a>, ReadError> {
            self.coverage()
        }

        fn substitutes(&self) -> Vec<GlyphId> {
            glyph_ids(self.substitute_glyph_ids())
        }

        fn backtrack(&self) -> Vec<CoverageTable<'a>> {
            self.backtrack_coverages().iter().flatten().collect()
        }

        fn lookahead(&self) -> Vec<CoverageTable<'a>> {
            self.lookahead_coverages().iter().flatten().collect()
        }
    }
}

delegate_to_formats! {
    impl<'a> ReverseChainSubstSource for ReverseChainSingleSubst {
        fn coverage(&self) -> Result<CoverageTable<'a>, ReadError>;
        fn substitutes(&self) -> Vec<GlyphId>;
        fn backtrack(&self) -> Vec<CoverageTable<'a>>;
        fn lookahead(&self) -> Vec<CoverageTable<'a>>;
    }
    [Format1, Format2]
}

/// The value records of a single positioning subtable.
pub(crate) enum SinglePosValues<'a> {
    /// One record applies to every covered glyph.
    Shared(ValueRecord<'a>),
    /// One record per covered glyph.
    PerGlyph(Vec<ValueRecord<'a>>),
}

pub(crate) trait SinglePosSource<'a> {
    fn coverage(&self) -> Result<CoverageTable<'a>, ReadError>;
    fn values(&self) -> SinglePosValues<'a>;
}

impl_for_widths! {
    impl<'a> SinglePosSource for [SinglePosFormat1, SinglePosFormat3] {
        fn coverage(&self) -> Result<CoverageTable<'a>, ReadError> {
            self.coverage()
        }

        fn values(&self) -> SinglePosValues<'a> {
            SinglePosValues::Shared(self.value_record())
        }
    }
}

impl_for_widths! {
    impl<'a> SinglePosSource for [SinglePosFormat2, SinglePosFormat4] {
        fn coverage(&self) -> Result<CoverageTable<'a>, ReadError> {
            self.coverage()
        }

        fn values(&self) -> SinglePosValues<'a> {
            SinglePosValues::PerGlyph(self.value_records().iter().flatten().collect())
        }
    }
}

delegate_to_formats! {
    impl<'a> SinglePosSource for SinglePos {
        fn coverage(&self) -> Result<CoverageTable<'a>, ReadError>;
        fn values(&self) -> SinglePosValues<'a>;
    }
    [Format1, Format2, Format3, Format4]
}

/// The positioning for the second glyph of a pair.
pub(crate) struct PairValue<'a> {
    pub second: GlyphId,
    pub first: ValueRecord<'a>,
    pub first_is_next: ValueRecord<'a>,
}

/// The positioning for one pair of classes.
pub(crate) struct ClassPairValue<'a> {
    pub first: ValueRecord<'a>,
    pub second: ValueRecord<'a>,
}

/// The class 2 records of one class 1, if that record reads at all. Positions
/// are kept for records which failed to read so that they stay aligned with
/// the class IDs.
pub(crate) type Class2Records<'a> = Option<Vec<Option<ClassPairValue<'a>>>>;

/// How a pair positioning subtable expresses the pairs it positions.
pub(crate) enum PairPositioning<'a> {
    /// One pair set per covered glyph.
    Pairs {
        coverage: Vec<GlyphId>,
        pair_sets: Vec<Vec<PairValue<'a>>>,
    },
    /// A matrix of values per pair of classes.
    Classes {
        coverage: Vec<GlyphId>,
        class_def1: ClassDef<'a>,
        class_def2: ClassDef<'a>,
        records: Vec<Class2Records<'a>>,
    },
}

pub(crate) trait PairPosSource<'a> {
    fn positioning(&self) -> Result<PairPositioning<'a>, ReadError>;
}

impl_for_widths! {
    impl<'a> PairPosSource for [PairPosFormat1, PairPosFormat3] {
        fn positioning(&self) -> Result<PairPositioning<'a>, ReadError> {
            Ok(PairPositioning::Pairs {
                coverage: self.coverage()?.iter().collect(),
                pair_sets: self
                    .pair_sets()
                    .iter()
                    .flatten()
                    .map(|set| {
                        set.pair_value_records()
                            .iter()
                            .flatten()
                            .map(|pair| PairValue {
                                second: pair.second_glyph().into(),
                                first: *pair.value_record1(),
                                first_is_next: *pair.value_record2(),
                            })
                            .collect()
                    })
                    .collect(),
            })
        }
    }
}

impl_for_widths! {
    impl<'a> PairPosSource for [PairPosFormat2, PairPosFormat4] {
        fn positioning(&self) -> Result<PairPositioning<'a>, ReadError> {
            Ok(PairPositioning::Classes {
                coverage: self.coverage()?.iter().collect(),
                class_def1: self.class_def1()?,
                class_def2: self.class_def2()?,
                records: self
                    .class1_records()
                    .iter()
                    .map(|record| {
                        let record = record.ok()?;
                        Some(
                            record
                                .class2_records()
                                .iter()
                                .map(|class2| {
                                    class2.ok().map(|class2| ClassPairValue {
                                        first: *class2.value_record1(),
                                        second: *class2.value_record2(),
                                    })
                                })
                                .collect(),
                        )
                    })
                    .collect(),
            })
        }
    }
}

delegate_to_formats! {
    impl<'a> PairPosSource for PairPos {
        fn positioning(&self) -> Result<PairPositioning<'a>, ReadError>;
    }
    [Format1, Format2, Format3, Format4]
}

/// The entry and exit anchors of one glyph.
pub(crate) type CursiveAnchors<'a> = Vec<(Option<AnchorTable<'a>>, Option<AnchorTable<'a>>)>;

pub(crate) trait CursivePosSource<'a> {
    fn coverage(&self) -> Result<CoverageTable<'a>, ReadError>;
    /// The entry and exit anchors of each covered glyph.
    fn anchors(&self) -> Result<CursiveAnchors<'a>, ReadError>;
}

impl_for_widths! {
    impl<'a> CursivePosSource for [CursivePosFormat1, CursivePosFormat2] {
        fn coverage(&self) -> Result<CoverageTable<'a>, ReadError> {
            self.coverage()
        }

        fn anchors(&self) -> Result<CursiveAnchors<'a>, ReadError> {
            let data = self.offset_data();
            self.entry_exit_record()
                .iter()
                .map(|record| {
                    Ok((
                        record.entry_anchor(data).transpose()?,
                        record.exit_anchor(data).transpose()?,
                    ))
                })
                .collect()
        }
    }
}

delegate_to_formats! {
    impl<'a> CursivePosSource for CursivePos {
        fn coverage(&self) -> Result<CoverageTable<'a>, ReadError>;
        fn anchors(&self) -> Result<CursiveAnchors<'a>, ReadError>;
    }
    [Format1, Format2]
}

/// The anchors of one target, indexed by mark class.
pub(crate) type AnchorRow<'a> = Vec<Option<AnchorTable<'a>>>;

pub(crate) trait MarkBasePosSource<'a> {
    fn base_coverage(&self) -> Result<CoverageTable<'a>, ReadError>;
    /// The mark glyphs, in coverage order, with their class and anchor.
    fn marks(&self) -> Result<Vec<(GlyphId, u16, AnchorTable<'a>)>, ReadError>;
    /// The anchors of each base glyph, indexed by mark class.
    fn base_anchors(&self) -> Result<Vec<AnchorRow<'a>>, ReadError>;
}

impl_for_widths! {
    impl<'a> MarkBasePosSource for [MarkBasePosFormat1, MarkBasePosFormat2] {
        fn base_coverage(&self) -> Result<CoverageTable<'a>, ReadError> {
            self.base_coverage()
        }

        fn marks(&self) -> Result<Vec<(GlyphId, u16, AnchorTable<'a>)>, ReadError> {
            let array = self.mark_array()?;
            Ok(self
                .mark_coverage()?
                .iter()
                .zip(array.mark_records())
                .filter_map(|(glyph, record)| {
                    record
                        .mark_anchor(array.offset_data())
                        .ok()
                        .map(|anchor| (glyph, record.mark_class(), anchor))
                })
                .collect())
        }

        fn base_anchors(&self) -> Result<Vec<AnchorRow<'a>>, ReadError> {
            let array = self.base_array()?;
            array
                .base_records()
                .iter()
                .flatten()
                .map(|record| {
                    record
                        .base_anchors(array.offset_data())
                        .iter()
                        .map(|anchor| anchor.transpose())
                        .collect()
                })
                .collect()
        }
    }
}

delegate_to_formats! {
    impl<'a> MarkBasePosSource for MarkBasePos {
        fn base_coverage(&self) -> Result<CoverageTable<'a>, ReadError>;
        fn marks(&self) -> Result<Vec<(GlyphId, u16, AnchorTable<'a>)>, ReadError>;
        fn base_anchors(&self) -> Result<Vec<AnchorRow<'a>>, ReadError>;
    }
    [Format1, Format2]
}

pub(crate) trait MarkLigPosSource<'a> {
    fn ligature_coverage(&self) -> Result<CoverageTable<'a>, ReadError>;
    /// The mark glyphs, in coverage order, with their class and anchor.
    fn marks(&self) -> Result<Vec<(GlyphId, u16, AnchorTable<'a>)>, ReadError>;
    /// The anchors of each component of each ligature, indexed by mark class.
    fn ligature_anchors(&self) -> Result<Vec<Vec<AnchorRow<'a>>>, ReadError>;
}

impl_for_widths! {
    impl<'a> MarkLigPosSource for [MarkLigPosFormat1, MarkLigPosFormat2] {
        fn ligature_coverage(&self) -> Result<CoverageTable<'a>, ReadError> {
            self.ligature_coverage()
        }

        fn marks(&self) -> Result<Vec<(GlyphId, u16, AnchorTable<'a>)>, ReadError> {
            let array = self.mark_array()?;
            Ok(self
                .mark_coverage()?
                .iter()
                .zip(array.mark_records())
                .filter_map(|(glyph, record)| {
                    record
                        .mark_anchor(array.offset_data())
                        .ok()
                        .map(|anchor| (glyph, record.mark_class(), anchor))
                })
                .collect())
        }

        fn ligature_anchors(&self) -> Result<Vec<Vec<AnchorRow<'a>>>, ReadError> {
            let array = self.ligature_array()?;
            array
                .ligature_attaches()
                .iter()
                .flatten()
                .map(|attach| {
                    attach
                        .component_records()
                        .iter()
                        .flatten()
                        .map(|component| {
                            component
                                .ligature_anchors(attach.offset_data())
                                .iter()
                                .map(|anchor| anchor.transpose())
                                .collect()
                        })
                        .collect()
                })
                .collect()
        }
    }
}

delegate_to_formats! {
    impl<'a> MarkLigPosSource for MarkLigPos {
        fn ligature_coverage(&self) -> Result<CoverageTable<'a>, ReadError>;
        fn marks(&self) -> Result<Vec<(GlyphId, u16, AnchorTable<'a>)>, ReadError>;
        fn ligature_anchors(&self) -> Result<Vec<Vec<AnchorRow<'a>>>, ReadError>;
    }
    [Format1, Format2]
}

pub(crate) trait MarkMarkPosSource<'a> {
    fn mark2_coverage(&self) -> Result<CoverageTable<'a>, ReadError>;
    /// The mark glyphs, in coverage order, with their class and anchor.
    fn marks(&self) -> Result<Vec<(GlyphId, u16, AnchorTable<'a>)>, ReadError>;
    /// The anchors of each mark2 glyph, indexed by mark class.
    fn mark2_anchors(&self) -> Result<Vec<AnchorRow<'a>>, ReadError>;
}

impl_for_widths! {
    impl<'a> MarkMarkPosSource for [MarkMarkPosFormat1, MarkMarkPosFormat2] {
        fn mark2_coverage(&self) -> Result<CoverageTable<'a>, ReadError> {
            self.mark2_coverage()
        }

        fn marks(&self) -> Result<Vec<(GlyphId, u16, AnchorTable<'a>)>, ReadError> {
            let array = self.mark1_array()?;
            Ok(self
                .mark1_coverage()?
                .iter()
                .zip(array.mark_records())
                .filter_map(|(glyph, record)| {
                    record
                        .mark_anchor(array.offset_data())
                        .ok()
                        .map(|anchor| (glyph, record.mark_class(), anchor))
                })
                .collect())
        }

        fn mark2_anchors(&self) -> Result<Vec<AnchorRow<'a>>, ReadError> {
            let array = self.mark2_array()?;
            array
                .mark2_records()
                .iter()
                .flatten()
                .map(|record| {
                    record
                        .mark2_anchors(array.offset_data())
                        .iter()
                        .map(|anchor| anchor.transpose())
                        .collect()
                })
                .collect()
        }
    }
}

delegate_to_formats! {
    impl<'a> MarkMarkPosSource for MarkMarkPos {
        fn mark2_coverage(&self) -> Result<CoverageTable<'a>, ReadError>;
        fn marks(&self) -> Result<Vec<(GlyphId, u16, AnchorTable<'a>)>, ReadError>;
        fn mark2_anchors(&self) -> Result<Vec<AnchorRow<'a>>, ReadError>;
    }
    [Format1, Format2]
}

/// A rule which matches a run of glyphs.
pub(crate) struct GlyphRule {
    /// The glyphs after the first, which comes from the coverage.
    pub input: Vec<GlyphId>,
    /// (index into the input sequence, index into the lookup list)
    pub lookups: Vec<(u16, u16)>,
}

/// A rule which matches a run of classes.
pub(crate) struct ClassRule {
    /// The classes after the first.
    pub input: Vec<u32>,
    pub lookups: Vec<(u16, u16)>,
}

/// A rule which matches a run of glyphs with context on either side.
pub(crate) struct ChainedGlyphRule {
    /// The backtrack glyphs, closest first.
    pub backtrack: Vec<GlyphId>,
    /// The glyphs after the first, which comes from the coverage.
    pub input: Vec<GlyphId>,
    pub lookahead: Vec<GlyphId>,
    pub lookups: Vec<(u16, u16)>,
}

/// A rule which matches a run of classes with context on either side.
pub(crate) struct ChainedClassRule {
    /// The backtrack classes, closest first.
    pub backtrack: Vec<u32>,
    /// The classes after the first.
    pub input: Vec<u32>,
    pub lookahead: Vec<u32>,
    pub lookups: Vec<(u16, u16)>,
}

/// The shapes a context subtable can take, with the formats which express the
/// same shape already merged.
pub(crate) enum SequenceContextData<'a> {
    /// One rule set per covered glyph.
    GlyphRules {
        coverage: Vec<GlyphId>,
        rule_sets: Vec<Vec<GlyphRule>>,
    },
    /// One rule set per input class.
    ClassRules {
        coverage: Vec<GlyphId>,
        class_def: ClassDef<'a>,
        rule_sets: Vec<Vec<ClassRule>>,
    },
    /// A single rule, expressed as one coverage table per input position.
    Coverages {
        coverages: Vec<CoverageTable<'a>>,
        lookups: Vec<(u16, u16)>,
    },
}

pub(crate) trait SequenceContextSource<'a> {
    fn data(&self) -> Result<SequenceContextData<'a>, ReadError>;
}

impl_for_widths! {
    impl<'a> SequenceContextSource for [SequenceContextFormat1, SequenceContextFormat4] {
        fn data(&self) -> Result<SequenceContextData<'a>, ReadError> {
            let mut rule_sets = vec![];
            for rule_set in self.seq_rule_sets().iter() {
                let Some(rule_set) = rule_set.transpose()? else {
                    rule_sets.push(vec![]);
                    continue;
                };
                rule_sets.push(
                    rule_set
                        .seq_rules()
                        .iter()
                        .flatten()
                        .map(|rule| GlyphRule {
                            input: glyph_ids(rule.input_sequence()),
                            lookups: lookup_records(rule.seq_lookup_records()),
                        })
                        .collect(),
                );
            }
            Ok(SequenceContextData::GlyphRules {
                coverage: self.coverage()?.iter().collect(),
                rule_sets,
            })
        }
    }
}

impl_for_widths! {
    impl<'a> SequenceContextSource for [SequenceContextFormat2, SequenceContextFormat5] {
        fn data(&self) -> Result<SequenceContextData<'a>, ReadError> {
            let mut rule_sets = vec![];
            for rule_set in self.class_seq_rule_sets().iter() {
                let Some(rule_set) = rule_set.transpose()? else {
                    rule_sets.push(vec![]);
                    continue;
                };
                rule_sets.push(
                    rule_set
                        .class_seq_rules()
                        .iter()
                        .flatten()
                        .map(|rule| ClassRule {
                            input: class_ids(rule.input_sequence()),
                            lookups: lookup_records(rule.seq_lookup_records()),
                        })
                        .collect(),
                );
            }
            Ok(SequenceContextData::ClassRules {
                coverage: self.coverage()?.iter().collect(),
                class_def: self.class_def()?,
                rule_sets,
            })
        }
    }
}

impl_for_widths! {
    impl<'a> SequenceContextSource for [SequenceContextFormat3, SequenceContextFormat6] {
        fn data(&self) -> Result<SequenceContextData<'a>, ReadError> {
            Ok(SequenceContextData::Coverages {
                coverages: self.coverages().iter().flatten().collect(),
                lookups: lookup_records(self.seq_lookup_records()),
            })
        }
    }
}

delegate_to_formats! {
    impl<'a> SequenceContextSource for SequenceContext {
        fn data(&self) -> Result<SequenceContextData<'a>, ReadError>;
    }
    [Format1, Format2, Format3, Format4, Format5, Format6]
}

/// The shapes a chained context subtable can take, with the formats which
/// express the same shape already merged.
pub(crate) enum ChainedSequenceContextData<'a> {
    /// One rule set per covered glyph.
    GlyphRules {
        coverage: Vec<GlyphId>,
        rule_sets: Vec<Vec<ChainedGlyphRule>>,
    },
    /// One rule set per input class.
    ClassRules {
        coverage: Vec<GlyphId>,
        input_class_def: Option<ClassDef<'a>>,
        backtrack_class_def: Option<ClassDef<'a>>,
        lookahead_class_def: Option<ClassDef<'a>>,
        rule_sets: Vec<Vec<ChainedClassRule>>,
    },
    /// A single rule, expressed as one coverage table per position.
    Coverages {
        backtrack: Vec<CoverageTable<'a>>,
        input: Vec<CoverageTable<'a>>,
        lookahead: Vec<CoverageTable<'a>>,
        lookups: Vec<(u16, u16)>,
    },
}

pub(crate) trait ChainedSequenceContextSource<'a> {
    fn data(&self) -> Result<ChainedSequenceContextData<'a>, ReadError>;
}

impl_for_widths! {
    impl<'a> ChainedSequenceContextSource for
        [ChainedSequenceContextFormat1, ChainedSequenceContextFormat4]
    {
        fn data(&self) -> Result<ChainedSequenceContextData<'a>, ReadError> {
            let mut rule_sets = vec![];
            for rule_set in self.chained_seq_rule_sets().iter() {
                let Some(rule_set) = rule_set.transpose()? else {
                    rule_sets.push(vec![]);
                    continue;
                };
                rule_sets.push(
                    rule_set
                        .chained_seq_rules()
                        .iter()
                        .flatten()
                        .map(|rule| ChainedGlyphRule {
                            backtrack: glyph_ids(rule.backtrack_sequence()),
                            input: glyph_ids(rule.input_sequence()),
                            lookahead: glyph_ids(rule.lookahead_sequence()),
                            lookups: lookup_records(rule.seq_lookup_records()),
                        })
                        .collect(),
                );
            }
            Ok(ChainedSequenceContextData::GlyphRules {
                coverage: self.coverage()?.iter().collect(),
                rule_sets,
            })
        }
    }
}

impl_for_widths! {
    impl<'a> ChainedSequenceContextSource for
        [ChainedSequenceContextFormat2, ChainedSequenceContextFormat5]
    {
        fn data(&self) -> Result<ChainedSequenceContextData<'a>, ReadError> {
            let mut rule_sets = vec![];
            for rule_set in self.chained_class_seq_rule_sets().iter() {
                let Some(rule_set) = rule_set.transpose()? else {
                    rule_sets.push(vec![]);
                    continue;
                };
                rule_sets.push(
                    rule_set
                        .chained_class_seq_rules()
                        .iter()
                        .flatten()
                        .map(|rule| ChainedClassRule {
                            backtrack: class_ids(rule.backtrack_sequence()),
                            input: class_ids(rule.input_sequence()),
                            lookahead: class_ids(rule.lookahead_sequence()),
                            lookups: lookup_records(rule.seq_lookup_records()),
                        })
                        .collect(),
                );
            }
            Ok(ChainedSequenceContextData::ClassRules {
                coverage: self.coverage()?.iter().collect(),
                input_class_def: self.input_class_def().ok(),
                backtrack_class_def: self.backtrack_class_def().ok(),
                lookahead_class_def: self.lookahead_class_def().ok(),
                rule_sets,
            })
        }
    }
}

impl<'a> ChainedSequenceContextSource<'a> for ChainedSequenceContextFormat3<'a> {
    fn data(&self) -> Result<ChainedSequenceContextData<'a>, ReadError> {
        Ok(ChainedSequenceContextData::Coverages {
            backtrack: self.backtrack_coverages().iter().flatten().collect(),
            input: self.input_coverages().iter().flatten().collect(),
            lookahead: self.lookahead_coverages().iter().flatten().collect(),
            lookups: lookup_records(self.seq_lookup_records()),
        })
    }
}

delegate_to_formats! {
    impl<'a> ChainedSequenceContextSource for ChainedSequenceContext {
        fn data(&self) -> Result<ChainedSequenceContextData<'a>, ReadError>;
    }
    [Format1, Format2, Format3, Format4, Format5]
}
