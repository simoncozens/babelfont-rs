use crate::filters::FontFilter;
mod encoded;

/// Recomposition filter: recomposes decomposed components.
pub struct Recompose;

impl FontFilter for Recompose {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        // We break this into multiple subfilters
        encoded::RecomposeEncoded::default().apply(font)
    }

    fn from_str(_s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(Self)
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("recompose")
            .long("recompose")
            .help("Recompose decomposed components")
            .action(clap::ArgAction::SetTrue)
    }
}
