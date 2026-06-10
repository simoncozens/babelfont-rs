use crate::filters::FontFilter;

/// A filter that moves explicit FEA-based kerning rules into the kerning table
#[derive(Default)]
pub struct MoveKerningFromFeatures;

impl MoveKerningFromFeatures {
    /// Create a new MoveKerningFromFeatures filter
    pub fn new() -> Self {
        MoveKerningFromFeatures
    }
}

impl FontFilter for MoveKerningFromFeatures {
    fn apply(&self, _font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        log::info!("Moving explicit FEA-based kerning rules into the kerning table");
        // Implementation for moving kerning rules goes here
        Ok(())
    }

    fn from_str(_s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(MoveKerningFromFeatures::new())
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("movekerningfromfeatures")
            .long("move-kerning-from-features")
            .help("Move explicit FEA-based kerning rules into the kerning table")
            .action(clap::ArgAction::SetTrue)
    }
}
