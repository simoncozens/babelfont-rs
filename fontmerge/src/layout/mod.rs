use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use fea_rs::{
    compile::NopVariationInfo,
    parse::{FileSystemResolver, SourceResolver},
    GlyphMap, ParseTree,
};

use crate::error::FontmergeError;

pub(crate) mod closure;
pub(crate) mod lookupgatherer;
pub(crate) mod subsetter;
pub(crate) mod visitor;

pub(crate) fn get_parse_tree(
    features: &babelfont::Features,
    glyph_names: &[&str],
    project_root: impl Into<PathBuf>,
) -> Result<ParseTree, FontmergeError> {
    let features = features.to_fea();
    let glyph_map = glyph_names
        .iter()
        .map(fontdrasil::types::GlyphName::new)
        .collect::<GlyphMap>();
    let resolver = FileSystemResolver::new(project_root.into());
    let features_text: Arc<str> = Arc::from(features);
    let (parse_tree, diagnostics) = fea_rs::parse::parse_root(
        "get_parse_tree".into(),
        Some(&glyph_map),
        Box::new(move |s: &Path| {
            if s == Path::new("get_parse_tree") {
                Ok(features_text.clone())
            } else {
                resolver.get_contents(s)
            }
        }),
    )?;
    if diagnostics.has_errors() {
        log::error!("Errors encountered while parsing feature file for layout closure:");
        log::error!("{}", diagnostics.display());
        return Err(FontmergeError::LayoutClosureError);
    }

    let diagnostics = fea_rs::compile::validate::<NopVariationInfo>(&parse_tree, &glyph_map, None);
    if diagnostics.has_errors() {
        log::error!("Errors encountered while validating feature file for layout closure:");
        log::error!("{}", diagnostics.display());
        return Err(FontmergeError::LayoutClosureError);
    }
    Ok(parse_tree)
}
