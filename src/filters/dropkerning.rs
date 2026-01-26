use crate::filters::FontFilter;

#[derive(Default)]
/// A filter that drops all kerning from a font
pub struct DropKerning;

impl DropKerning {
    /// Create a new DropKerning filter
    pub fn new() -> Self {
        DropKerning
    }
}

impl FontFilter for DropKerning {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        log::info!("Dropping all kerning from font");
        for master in font.masters.iter_mut() {
            master.kerning.clear();
        }
        Ok(())
    }

    fn from_str(_s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(DropKerning::new())
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("dropkerning")
            .long("drop-kerning")
            .help("Drop all kerning data from the font")
            .action(clap::ArgAction::SetTrue)
    }
}
