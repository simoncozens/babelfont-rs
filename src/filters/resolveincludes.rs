use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use fea_rs_ast::fea_rs;

use crate::filters::FontFilter;

/// A filter that resolves include statements in feature files
pub struct ResolveIncludes(Option<PathBuf>);

impl ResolveIncludes {
    /// Create a new ResolveIncludes filter
    ///
    /// The optional path is used as the base path for resolving includes.
    /// If this is not present, the font's source path will be used instead.
    pub fn new(path: Option<impl Into<PathBuf>>) -> Self {
        ResolveIncludes(path.map(|a| a.into()))
    }
}

impl FontFilter for ResolveIncludes {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        if let Some(base_path) = &self.0 {
            log::info!("Resolving includes with base path: {:?}", base_path);
            resolve_includes(font, base_path.to_path_buf())
        } else if let Some(source_path) = &font.source.clone() {
            let base_path = source_path.parent().ok_or_else(|| {
                crate::BabelfontError::FilterError("Cannot determine base path".into())
            })?;
            log::info!(
                "Resolving includes with font source base path: {:?}",
                base_path
            );
            resolve_includes(font, source_path.into())
        } else {
            Err(crate::BabelfontError::FilterError(
                "No base path provided and font has no source path".into(),
            ))
        }
    }
}

fn resolve_includes(
    font: &mut crate::Font,
    base_path: PathBuf,
) -> Result<(), crate::BabelfontError> {
    let glyph_names = font
        .glyphs
        .iter()
        .map(|gl| gl.name.clone())
        .collect::<Vec<_>>();
    let glyph_map = fea_rs::GlyphMap::from_iter(glyph_names.iter().cloned());
    let mut paths = vec![base_path.clone()];
    paths.extend(font.features.include_paths.iter().cloned());
    for prefix in font.features.prefixes.values_mut() {
        if !prefix.contains("include(") {
            continue;
        }
        let paths = paths.clone();
        let mut resolvers: Vec<Box<dyn fea_rs::parse::SourceResolver>> = paths
            .iter()
            .map(|base_path| {
                Box::new(fea_rs::parse::FileSystemResolver::new(base_path.clone()))
                    as Box<dyn fea_rs::parse::SourceResolver>
            })
            .collect();
        // If we have any additional include paths, add them to the resolver
        for include_path in &font.features.include_paths {
            resolvers.push(Box::new(fea_rs::parse::FileSystemResolver::new(
                include_path.clone(),
            )));
        }

        let features_text: Arc<str> = Arc::from(prefix.as_str());
        let (parse_tree, mut diagnostics) = fea_rs::parse::parse_root(
            "get_parse_tree".into(),
            Some(&glyph_map),
            Box::new(move |s: &Path| {
                if s == Path::new("get_parse_tree") {
                    Ok(features_text.clone())
                } else {
                    log::info!("Resolving include: {}", s.display());
                    resolvers
                        .iter()
                        .find_map(|r| {
                            let path = r.resolve_raw_path(s.as_ref(), None);
                            let canonical = r.canonicalize(&path).ok()?;
                            r.get_contents(&canonical).ok()
                        })
                        .ok_or_else(|| {
                            fea_rs::parse::SourceLoadError::new(
                                s.to_path_buf(),
                                format!(
                                    "File not found in include paths {}",
                                    paths
                                        .iter()
                                        .map(|p| p.display().to_string())
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                ),
                            )
                        })
                }
            }),
        )
        .map_err(|e| {
            crate::BabelfontError::FilterError(format!(
                "Failed to parse feature file for includes: {}",
                e
            ))
        })?;
        diagnostics.split_off_warnings();
        if diagnostics.has_errors() {
            return Err(crate::BabelfontError::FilterError(format!(
                "Error resolving includes: {:?}",
                diagnostics
            )));
        }
        // Reconstruct the feature file without includes
        let mut new_prefix = String::new();
        for token in parse_tree.root().iter_tokens() {
            new_prefix.push_str(token.as_str());
        }
        *prefix = new_prefix;
    }
    Ok(())
}
