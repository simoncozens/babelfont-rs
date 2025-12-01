use crate::Font;
use fontdrasil::orchestration::{Access, Work};
use fontir::{
    error::Error,
    orchestration::{Context, WorkId},
};
use std::sync::Arc;

#[derive(Debug)]
pub(crate) struct ColorPaletteWork {
    pub _font: Arc<Font>,
}

impl Work<Context, WorkId, Error> for ColorPaletteWork {
    fn id(&self) -> WorkId {
        WorkId::ColorPalettes
    }

    fn read_access(&self) -> Access<WorkId> {
        Access::None
    }

    fn write_access(&self) -> Access<WorkId> {
        Access::Variant(WorkId::ColorPalettes)
    }

    fn exec(&self, _context: &Context) -> Result<(), Error> {
        // We do nothing for now
        Ok(())
    }
}

#[derive(Debug)]
pub(crate) struct PaintGraphWork {
    pub _font: Arc<Font>,
}

impl Work<Context, WorkId, Error> for PaintGraphWork {
    fn id(&self) -> WorkId {
        WorkId::PaintGraph
    }

    fn read_access(&self) -> Access<WorkId> {
        Access::None
    }

    fn write_access(&self) -> Access<WorkId> {
        Access::Variant(WorkId::PaintGraph)
    }

    fn exec(&self, _context: &Context) -> Result<(), Error> {
        log::debug!("TODO: actually create paint graph");
        Ok(())
    }
}
