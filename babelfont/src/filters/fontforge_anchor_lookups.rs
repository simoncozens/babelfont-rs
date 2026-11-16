//! FontForge's anchor lookups, as the SFD reader records them for
//! `--fontforge-mark-lookups`.
//!
//! An SFD mark-to-base, mark-to-ligature, mark-to-mark or cursive lookup has no rules
//! of its own: each of its subtables owns anchor classes, and the glyphs' anchor
//! points of those classes are its data. The reader keeps every such lookup in
//! [`KEY`] on the font, in SFD declaration order.

use serde::{Deserialize, Serialize};

/// The font `format_specific` key holding a `Vec<AnchorLookup>`.
pub(crate) const KEY: &str = "sfd.anchor_lookups";

/// The anchor `format_specific` key holding the anchor's SFD class name.
pub(crate) const ANCHOR_CLASS_KEY: &str = "sfd.class";

/// The kind of attachment an anchor lookup makes.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AnchorLookupKind {
    Cursive,
    MarkToBase,
    MarkToLigature,
    MarkToMark,
}

/// One feature, script and language a lookup is registered for.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub(crate) struct LookupRegistration {
    pub(crate) feature: String,
    pub(crate) script: String,
    pub(crate) language: String,
}

/// One SFD anchor lookup.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub(crate) struct AnchorLookup {
    /// The lookup's feature-file name.
    pub(crate) name: String,
    pub(crate) kind: AnchorLookupKind,
    /// The SFD lookup flags.
    pub(crate) flag: u16,
    /// The glyphs of the mark attachment class the flag names, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) mark_attachment_class: Option<Vec<String>>,
    /// The glyphs of the mark filtering set the flag names, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) mark_filtering_set: Option<Vec<String>>,
    /// The anchor classes of each subtable, in order.
    pub(crate) subtables: Vec<Vec<String>>,
    pub(crate) registrations: Vec<LookupRegistration>,
    /// The feature prefix of the next GPOS lookup the reader wrote out, if any: the
    /// lookup's definition goes before it to keep the declaration order.
    pub(crate) before: Option<String>,
}
