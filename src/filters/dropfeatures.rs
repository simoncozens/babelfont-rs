use crate::filters::FontFilter;

/// A filter that drops all features from a font
#[derive(Default)]
pub struct DropFeatures;

impl DropFeatures {
    /// Create a new DropFeatures filter
    pub fn new() -> Self {
        DropFeatures
    }
}

impl FontFilter for DropFeatures {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        log::info!("Dropping all features from font");
        font.features = crate::Features::default();
        Ok(())
    }

    fn from_str(_s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(DropFeatures::new())
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("dropfeatures")
            .long("drop-features")
            .help("Drop all OpenType features from the font")
            .action(clap::ArgAction::SetTrue)
    }
}
