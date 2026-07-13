use crate::{Font, Master};

/// Options controlling how much of a font [`crate::load_with_options`] loads.
///
/// Every flag defaults to `true`, which loads the whole font and matches
/// [`crate::load`]. Switching flags off skips the corresponding ingestion
/// work, which can make loading large fonts substantially faster — most
/// notably for designspace/UFO fonts, where skipping glyph layers avoids
/// parsing any `.glif` file, and skipping masters avoids opening the source
/// UFOs at all. Single-file formats still have to be parsed in full, but
/// skip the conversion of the parts that were not requested.
///
/// ```
/// use babelfont::LoadOptions;
///
/// let metadata_only = LoadOptions {
///     load_layers: false,
///     ..Default::default()
/// };
/// ```
#[derive(Debug, Clone)]
pub struct LoadOptions {
    /// Populate [`Font::axes`](crate::Font). When `false`, the returned font
    /// has no axes.
    pub load_axes: bool,
    /// Load the content of each master: metrics, kerning, guides, and custom
    /// data. When `false`, the font's masters are bare records of what the
    /// file declares — name, id, and location (plus, for designspace sources,
    /// the filename and style name) — and formats that store masters in
    /// separate files do not open those files at all. Implies
    /// `load_layers: false`, since layers belong to masters.
    pub load_masters: bool,
    /// Populate [`Font::glyphs`](crate::Font). When `false`, the returned
    /// font has no glyphs. (Glyphs-format fonts store kerning-group
    /// membership on their glyphs, so kerning groups are lost too; use
    /// `load_layers: false` instead if you need them.)
    pub load_glyphs: bool,
    /// Load glyph layers (outlines, anchors, and other per-layer data). When
    /// `false`, glyphs are stubs without layers: for UFO and designspace
    /// fonts their names are read from the default layer's `contents.plist`,
    /// so codepoints — which live in the unparsed `.glif` files — are not
    /// available; Glyphs-format stubs keep codepoints and the rest of their
    /// glyph-level data. Only effective when `load_masters` and `load_glyphs`
    /// are `true`.
    pub load_layers: bool,
}

impl Default for LoadOptions {
    fn default() -> Self {
        LoadOptions {
            load_axes: true,
            load_masters: true,
            load_glyphs: true,
            load_layers: true,
        }
    }
}

impl LoadOptions {
    /// Drop the parts of a fully-loaded font that these options exclude.
    ///
    /// Used for formats whose convertors cannot skip the work at load time,
    /// so that the result has the same shape for every format.
    pub(crate) fn filter_loaded_font(&self, font: &mut Font) {
        if !self.load_axes {
            font.axes.clear();
        }
        if !self.load_glyphs {
            font.glyphs.clear();
        }
        if !self.load_masters {
            font.masters = font
                .masters
                .iter()
                .map(|m| Master::new(m.name.clone(), m.id.clone(), m.location.clone()))
                .collect();
        }
        if !(self.load_masters && self.load_layers) {
            for g in font.glyphs.iter_mut() {
                g.layers.clear();
            }
        }
    }
}

/// A constituent of a font that failed to load, reported by
/// [`crate::load_with_options`].
#[derive(Debug, Clone, serde::Serialize)]
pub struct SourceLoadFailure {
    /// The name of the source as the font declares it (for a designspace,
    /// the `filename` attribute of the `<source>` element).
    pub filename: String,
    /// The error that prevented the source from loading.
    pub error: String,
}

/// The result of [`crate::load_with_options`].
#[derive(Debug)]
pub struct LoadResult {
    /// The loaded font.
    pub font: Font,
    /// Sources that failed to load and were skipped: each failure leaves the
    /// returned font without the corresponding master's content. Always empty
    /// for single-file formats, which either load or fail outright.
    pub source_failures: Vec<SourceLoadFailure>,
}

impl From<Font> for LoadResult {
    /// Wrap a font that loaded without any source failures.
    fn from(font: Font) -> Self {
        LoadResult {
            font,
            source_failures: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use crate::LoadOptions;

    #[test]
    fn options_filter_formats_without_native_support() {
        let full = crate::load("resources/RadioCanadaDisplay.babelfont").unwrap();
        let result = crate::load_with_options(
            "resources/RadioCanadaDisplay.babelfont",
            &LoadOptions {
                load_glyphs: false,
                load_masters: false,
                ..Default::default()
            },
        )
        .unwrap();
        let font = result.font;
        assert!(result.source_failures.is_empty());
        assert!(font.glyphs.is_empty());
        assert_eq!(font.masters.len(), full.masters.len());
        // Bare master records: identity and location, but no content
        assert_eq!(font.masters[0].location, full.masters[0].location);
        assert!(font.masters[0].kerning.is_empty());
        assert!(font.masters[0].metrics.is_empty());
    }
}
