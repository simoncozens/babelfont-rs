use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use babelfont::SmolStr;
use fea_rs_ast::{
    fea_rs,
    fea_rs::{
        compile::NopVariationInfo,
        parse::{FileSystemResolver, SourceResolver},
        typed::{AstNode, GlyphOrClass},
        GlyphMap, Kind, ParseTree,
    },
};
use itertools::Either;

use crate::error::FontmergeError;

pub(crate) mod lookupgatherer;
pub(crate) mod visitor;

pub(crate) fn get_parse_tree(
    features: &str,
    glyph_names: &[&SmolStr],
    project_root: impl Into<PathBuf>,
) -> Result<ParseTree, FontmergeError> {
    let glyph_map = glyph_names.iter().cloned().collect::<GlyphMap>();
    let resolver: FileSystemResolver = FileSystemResolver::new(project_root.into());
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
