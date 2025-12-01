use std::sync::Arc;

use fontdrasil::orchestration::Work;
use fontir::{
    error::Error,
    ir::FeaturesSource,
    orchestration::{Context, WorkId},
};

use crate::Font;

#[derive(Debug)]
pub struct FeatureWork {
    pub font: Arc<Font>,
}

impl Work<Context, WorkId, Error> for FeatureWork {
    fn id(&self) -> WorkId {
        WorkId::Features
    }

    fn exec(&self, context: &Context) -> Result<(), Error> {
        log::trace!("Generate features");
        let include_dir = self
            .font
            .source
            .as_ref()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf());

        context.features.set(to_ir_features(
            &Some(self.font.features.to_fea()),
            include_dir,
        )?);
        Ok(())
    }
}

pub(crate) fn to_ir_features(
    features: &Option<String>,
    include_dir: Option<std::path::PathBuf>,
) -> Result<FeaturesSource, Error> {
    // Based on https://github.com/googlefonts/glyphsLib/blob/24b4d340e4c82948ba121dcfe563c1450a8e69c9/Lib/glyphsLib/builder/features.py#L74
    // TODO: token expansion
    // TODO: implement notes
    Ok(FeaturesSource::Memory {
        fea_content: features.clone().unwrap_or_default(),
        include_dir,
    })
}
