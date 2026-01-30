use crate::{
    convertors::fontir::varc::insert_varc_table,
    filters::{DropIncompatiblePaths, FontFilter as _, RetainGlyphs, RewriteSmartAxes},
    BabelfontError, Font,
};
use fontc::Options;
use fontdrasil::{
    coords::{NormalizedCoord, NormalizedLocation},
    orchestration::Work,
    types::GlyphName,
};
use fontir::{
    error::Error,
    ir::KerningGroups,
    orchestration::{Context, IrWork, WorkId},
    source::Source,
};
use std::sync::Arc;

mod color;
mod features;
mod global_metrics;
mod glyphs;
mod kerning;
mod static_metadata;
mod varc;

/// Options for compiling a Babelfont Font to FontIR
#[derive(Debug, Clone)]
pub struct CompilationOptions {
    /// Skip kerning generation
    pub skip_kerning: bool,
    /// Skip feature generation
    pub skip_features: bool,
    /// Skip metrics generation
    pub skip_metrics: bool,
    /// Skip outline generation
    pub skip_outlines: bool,
    /// Do not use production names for glyphs
    pub dont_use_production_names: bool,
    /// Produce VARC table (defaults to true)
    pub produce_varc_table: bool,
    /// Drop incompatible paths from layers
    pub drop_incompatible_paths: bool,
}

impl Default for CompilationOptions {
    fn default() -> Self {
        Self {
            skip_kerning: false,
            skip_features: false,
            skip_metrics: false,
            skip_outlines: false,
            dont_use_production_names: false,
            produce_varc_table: true,
            drop_incompatible_paths: false,
        }
    }
}

#[derive(Debug, Clone)]
/// A FontIR source for a Babelfont Font
pub struct BabelfontIrSource {
    font: Arc<Font>,
    options: CompilationOptions,
}

impl BabelfontIrSource {
    fn create_work_for_one_glyph(
        &self,
        glyph_name: GlyphName,
    ) -> Result<glyphs::GlyphIrWork, Error> {
        Ok(glyphs::GlyphIrWork {
            glyph_name,
            font: self.font.clone(),
            options: self.options.clone(),
        })
    }

    /// Create a new BabelfontIrSource that the user will compile themselves
    pub fn new(font: Font, options: CompilationOptions) -> Self {
        Self {
            font: Arc::new(font),
            options,
        }
    }

    /// Compile the Babelfont Font to a font binary
    pub fn compile(font: Font, options: CompilationOptions) -> Result<Vec<u8>, BabelfontError> {
        let mut font = font.clone();
        assert!(!font.masters.is_empty());
        if options.drop_incompatible_paths {
            DropIncompatiblePaths.apply(&mut font)?;
        }

        if options.produce_varc_table {
            RewriteSmartAxes.apply(&mut font)?;
        }
        // Unexported glyphs - decompose and drop
        RetainGlyphs::new(
            font.glyphs
                .iter()
                .filter(|g| g.exported)
                .map(|g| g.name.to_string())
                .collect(),
        )
        .apply(&mut font)?;
        assert!(
            !font.masters.is_empty(),
            "No masters remain after filtering"
        );
        // Make sure we have some exported glyphs
        assert!(
            font.glyphs.iter().any(|g| g.exported),
            "No exported glyphs remain after filtering"
        );
        let source = Self {
            font: Arc::new(font),
            options,
        };
        let binary = fontc::generate_font(Box::new(source.clone()), Options::default())
            .map_err(|e| BabelfontError::General(format!("Font generation error: {:#?}", e)))?;
        if source.options.produce_varc_table {
            insert_varc_table(&binary, &source.font)
        } else {
            Ok(binary)
        }
    }
}

impl Source for BabelfontIrSource {
    fn new(_unused: &std::path::Path) -> Result<Self, Error> {
        unimplemented!();
    }

    fn create_static_metadata_work(&self) -> Result<Box<IrWork>, Error> {
        Ok(Box::new(static_metadata::StaticMetadataWork(self.clone())))
    }

    fn create_global_metric_work(&self) -> Result<Box<IrWork>, Error> {
        if self.options.skip_metrics {
            return Ok(Box::new(DummyWork(WorkId::GlobalMetrics)));
        }
        Ok(Box::new(global_metrics::GlobalMetricWork(
            self.font.clone(),
        )))
    }

    fn create_glyph_ir_work(&self) -> Result<Vec<Box<IrWork>>, fontir::error::Error> {
        self.font
            .glyphs
            .iter()
            // .filter(|g| g.exported)
            .map(|glyph| {
                self.create_work_for_one_glyph(glyph.name.clone().into())
                    .map(|w| -> Box<IrWork> { Box::new(w) })
            })
            .collect()
    }

    fn create_feature_ir_work(&self) -> Result<Box<IrWork>, Error> {
        if self.options.skip_features {
            return Ok(Box::new(DummyWork(WorkId::Features)));
        }
        Ok(Box::new(features::FeatureWork {
            font: self.font.clone(),
        }))
    }

    fn create_kerning_group_ir_work(&self) -> Result<Box<IrWork>, Error> {
        if self.options.skip_kerning {
            return Ok(Box::new(DummyWork(WorkId::KerningGroups)));
        }

        Ok(Box::new(kerning::KerningGroupWork(self.font.clone())))
    }

    fn create_kerning_instance_ir_work(
        &self,
        at: NormalizedLocation,
    ) -> Result<Box<IrWork>, Error> {
        if self.options.skip_kerning {
            return Ok(Box::new(DummyWork(WorkId::KernInstance(at.clone()))));
        }
        Ok(Box::new(kerning::KerningInstanceWork {
            font: self.font.clone(),
            location: at,
        }))
    }

    fn create_color_palette_work(
        &self,
    ) -> Result<Box<fontir::orchestration::IrWork>, fontir::error::Error> {
        Ok(Box::new(color::ColorPaletteWork {
            _font: self.font.clone(),
        }))
    }

    fn create_color_glyphs_work(&self) -> Result<Box<IrWork>, Error> {
        Ok(Box::new(color::PaintGraphWork {
            _font: self.font.clone(),
        }))
    }
}

#[derive(Debug)]
struct DummyWork(WorkId);
impl Work<Context, WorkId, Error> for DummyWork {
    fn id(&self) -> WorkId {
        self.0.clone()
    }

    fn exec(&self, context: &Context) -> Result<(), Error> {
        match &self.0 {
            WorkId::StaticMetadata => todo!(),
            WorkId::GlobalMetrics => todo!(),
            WorkId::Glyph(_glyph_name) => todo!(),
            WorkId::PreliminaryGlyphOrder => todo!(),
            WorkId::GlyphOrder => todo!(),
            WorkId::Features => context.features.set(fontir::ir::FeaturesSource::Memory {
                fea_content: String::new(),
                include_dir: None,
            }),
            WorkId::KerningGroups => context.kerning_groups.set(KerningGroups::default()),
            WorkId::KernInstance(location) => context.kerning_at.set(fontir::ir::KerningInstance {
                location: location.clone(),
                ..Default::default()
            }),
            WorkId::Anchor(_glyph_name) => todo!(),
            WorkId::ColorPalettes => todo!(),
            WorkId::PaintGraph => todo!(),
        }
        Ok(())
    }
}

pub(crate) fn debug_location(loc: &NormalizedLocation) -> String {
    let mut loc2 = loc.clone();
    loc2.retain(|_tag, coord| *coord != NormalizedCoord::new(0.0));
    format!("{:?}", loc2)
}
