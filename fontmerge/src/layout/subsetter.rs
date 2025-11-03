use indexmap::IndexSet;

pub(crate) struct LayoutSubsetter<'a> {
    features: &'a babelfont::Features,
    final_glyphset: &'a Vec<String>,
    hidden_classes: &'a IndexSet<String>,
    pre_existing_lookups: &'a IndexSet<String>,
}

impl LayoutSubsetter<'_> {
    pub fn new<'a>(
        features: &'a babelfont::Features,
        final_glyphset: &'a Vec<String>,
        hidden_classes: &'a IndexSet<String>,
        pre_existing_lookups: &'a IndexSet<String>,
    ) -> LayoutSubsetter<'a> {
        LayoutSubsetter {
            features,
            final_glyphset,
            hidden_classes,
            pre_existing_lookups,
        }
    }

    pub fn subset(&mut self) -> Result<babelfont::Features, crate::error::FontmergeError> {
        // Placeholder implementation
        Ok(self.features.clone())
    }
}
