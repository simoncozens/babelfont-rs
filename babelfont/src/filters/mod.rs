/// Macro to declare filters with less boilerplate
///
/// Usage: `declare_filters! { TypeName(module_name) => "cli_name", ... }`
macro_rules! declare_filters {
    ($($(#[$meta:meta])* $type:ident($module:ident) => $name:literal),* $(,)?) => {
        // Import modules
        $(
            $(#[$meta])*
            mod $module;
        )*

        // Re-export types
        $(
            $(#[$meta])*
            pub use $module::$type;
        )*

        // Generate filter_group function
        #[cfg(feature = "cli")]
        #[doc="Add filter arguments to a clap Command"]
        pub fn filter_group(mut command: clap::Command) -> clap::Command {
            command = command.next_help_heading("Font filters");
            let mut ids = Vec::new();
            $(
                $(#[$meta])*
                {
                    let arg = $type::arg();
                    ids.push(arg.get_id().clone());
                    command = command.arg(arg);
                }
            )*
            command.group(clap::ArgGroup::new("filters").args(ids).multiple(true))
        }

        // Generate cli_to_filter function
        #[cfg(feature = "cli")]
        #[doc="Convert a CLI filter name and argument to a FontFilter instance"]
        pub fn cli_to_filter(name: &str, arg: &str) -> Result<Box<dyn FontFilter>, crate::BabelfontError> {
            Ok(match name {
                $(
                    $(#[$meta])*
                    $name => Box::new($type::from_str(arg)?),
                )*
                _ => {
                    return Err(crate::BabelfontError::FilterError(format!(
                        "Unknown filter: {}",
                        name
                    )))
                }
            })
        }
    };
}

// Declare all filters in one place
declare_filters! {
    DropKerning(dropkerning) => "dropkerning",
    DropGuides(dropguides) => "dropguides",
    DropFeatures(dropfeatures) => "dropfeatures",
    DropInstances(dropinstances) => "dropinstances",
    DropVariations(dropvariations) => "dropvariations",
    DropAxis(dropaxis) => "dropaxis",
    DropSparseMasters(dropsparsemasters) => "dropsparsemasters",
    ResolveIncludes(resolveincludes) => "resolveincludes",
    RetainGlyphs(retainglyphs) => "retainglyphs",
    DecomposeComponentReferences(decomposecomponentreferences) => "decomposecomponents",
    RewriteSmartAxes(rewritesmartaxes) => "rewritesmartaxes",
    ScaleUpem(scaleupem) => "scaleupem",
    SubsetLayout(subsetlayout) => "subsetlayout",
    #[cfg(feature = "glyphs")]
    GlyphsData(glyphsdata) => "glyphsdata",
    DropIncompatiblePaths(dropincompatiblepaths) => "dropincompatiblepaths",
    GlyphsNumberValue(glyphsnumbervalue) => "glyphsnumbervalue",
}

/// A trait for font filters that can be applied to a font
pub trait FontFilter {
    /// Apply the filter to the given font
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError>;

    /// Parse a FontFilter from a string argument
    fn from_str(s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized;

    #[cfg(feature = "cli")]
    /// Get the clap argument for this filter
    fn arg() -> clap::Arg
    where
        Self: Sized;
}
