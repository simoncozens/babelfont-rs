use thiserror::Error;
/// Error type for fontmerge
#[derive(Error, Debug)]
pub enum FontmergeError {
    /// IO errors
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Font parsing errors
    #[error("Font error: {0}")]
    Font(String),

    /// Parsing errors
    #[error("Parse error: {0}")]
    Parse(String),

    /// Interpolation errors
    #[error("Interpolation error: {0}")]
    Interpolation(String),

    /// Layout closure errors
    #[error("Glyphset grew unreasonably during layout closure")]
    LayoutClosureError,

    /// Source loading error
    #[error("Source loading error: {0}")]
    SourceLoadError(#[from] fea_rs::parse::SourceLoadError),
}
