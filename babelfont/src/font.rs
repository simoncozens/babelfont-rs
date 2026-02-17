use crate::{
    axis::{Axis, CrossAxisMapping, Tag},
    common::{CustomOTValues, FormatSpecific},
    features::Features,
    glyph::GlyphList,
    instance::Instance,
    master::Master,
    names::Names,
    BabelfontError, Layer, LayerType, MetricType,
};
use fontdrasil::coords::{
    DesignCoord, DesignLocation, DesignSpace, Location, NormalizedLocation, NormalizedSpace,
    UserCoord,
};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;
use std::{collections::BTreeMap, path::PathBuf};
use typeshare::typeshare;

#[cfg(feature = "cli")]
extern crate serde_json_path_to_error as serde_json;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[typeshare]
/// A representation of a font source file
pub struct Font {
    /// Units per em
    pub upm: u16,
    /// Font version as (major, minor)
    #[typeshare(python(type = "Tuple[int, int]"))]
    #[typeshare(typescript(type = "[number, number]"))]
    pub version: (u16, u16),
    /// A list of axes, in the case of variable/multiple master font.
    ///
    /// May be empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub axes: Vec<Axis>,
    /// A list of cross-axis mappings (avar2 mappings)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cross_axis_mappings: Vec<CrossAxisMapping>,
    /// A list of named/static instances
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub instances: Vec<Instance>,
    /// A list of the font's masters
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub masters: Vec<Master>,
    /// A list of the font's glyphs
    #[typeshare(serialized_as = "Vec<Glyph>")]
    pub glyphs: GlyphList,
    /// An optional note about the font
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// The font's creation date
    #[typeshare(python(type = "datetime.datetime"))]
    #[typeshare(typescript(type = "Date"))]
    pub date: chrono::DateTime<chrono::Utc>,
    /// The font's naming information
    pub names: Names,
    /// Any values to be placed in OpenType tables on export to override defaults
    ///
    /// These must be font-wide. Metrics which may vary by master should be placed in the `metrics` field of a Master
    #[serde(default, skip_serializing_if = "CustomOTValues::is_empty")]
    pub custom_ot_values: CustomOTValues,
    /// A map of Unicode Variation Sequences to glyph names
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[typeshare(python(type = "Dict[Tuple[int, int], str]"))]
    #[typeshare(typescript(type = "Record<string, string>"))]
    pub variation_sequences: BTreeMap<(u32, u32), SmolStr>,
    /// A representation of the font's OpenType features
    pub features: Features,
    /// A dictionary of kerning groups
    ///
    /// The key is the group name and the value is a list of glyph names in the group
    /// Group names are *not* prefixed with "@" here. This is the first item in a kerning pair.
    /// and so these are generally organized based on the profile of *right side* of the
    /// glyph (for LTR scripts).
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    #[typeshare(python(type = "Dict[str, List[str]]"))]
    #[typeshare(typescript(type = "Record<string, string[]>"))]
    pub first_kern_groups: IndexMap<SmolStr, Vec<SmolStr>>,
    // A dictionary of kerning groups
    ///
    /// The key is the group name and the value is a list of glyph names in the group
    /// Group names are *not* prefixed with "@" here. This is the second item in a kerning pair.
    /// and so these are generally organized based on the profile of *left side* of the
    /// glyph (for LTR scripts).
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    #[typeshare(serialized_as = "HashMap<String, Vec<String>>")]
    pub second_kern_groups: IndexMap<SmolStr, Vec<SmolStr>>,

    /// Format-specific data
    #[serde(default, skip_serializing_if = "FormatSpecific::is_empty")]
    #[typeshare(python(type = "Dict[str, Any]"))]
    #[typeshare(typescript(type = "Record<string, any>"))]
    pub format_specific: FormatSpecific,

    /// The source file path, if any, from which this font was loaded
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[typeshare(serialized_as = "Option<String>")]
    pub source: Option<PathBuf>,
}
impl Default for Font {
    fn default() -> Self {
        Self::new()
    }
}

impl Font {
    /// Create a new, empty Font
    pub fn new() -> Self {
        Font {
            upm: 1000,
            version: (1, 0),
            // We use a lot of Default::default()s here to avoid having
            // to change this if we change the type of any of these fields.
            axes: Default::default(),
            cross_axis_mappings: Default::default(),
            instances: Default::default(),
            masters: Default::default(),
            glyphs: Default::default(),
            note: None,
            date: chrono::Utc::now(),
            names: Default::default(),
            custom_ot_values: Default::default(),
            variation_sequences: Default::default(),
            first_kern_groups: Default::default(),
            second_kern_groups: Default::default(),
            features: Default::default(),
            format_specific: Default::default(),
            source: None,
        }
    }

    /// Find the location of the default master in design space coordinates, if one is present
    pub fn default_location(&self) -> Result<DesignLocation, BabelfontError> {
        let iter: Result<Vec<(Tag, DesignCoord)>, _> = self
            .axes
            .iter()
            .map(|axis| {
                axis.userspace_to_designspace(axis.default.unwrap_or(UserCoord::new(0.0)))
                    .map(|coord| (axis.tag, coord))
            })
            .collect();
        Ok(DesignLocation::from_iter(iter?))
    }

    /// Find the default master, if one is present
    pub fn default_master(&self) -> Option<&Master> {
        let default_location: DesignLocation = self.default_location().ok()?;
        if self.masters.len() == 1 {
            return Some(&self.masters[0]);
        }
        self.masters
            .iter()
            .find(|&m| m.location == default_location)
    }

    /// Find the index of the default master, if one is present
    pub fn default_master_index(&self) -> Option<usize> {
        let default_location: DesignLocation = self.default_location().ok()?;
        self.masters
            .iter()
            .enumerate()
            .find_map(|(ix, m)| (m.location == default_location).then_some(ix))
    }

    /// Find a master by its name
    pub fn master(&self, master_name: &str) -> Option<&Master> {
        self.masters
            .iter()
            .find(|m| m.name.get_default().map(|x| x.as_str()) == Some(master_name))
    }

    /// Find the layer for a given glyph and master, if it exists
    pub fn master_layer_for(&self, glyphname: &str, master: &Master) -> Option<&Layer> {
        if let Some(glyph) = self.glyphs.get(glyphname) {
            for layer in &glyph.layers {
                if layer.master == LayerType::DefaultForMaster(master.id.clone()) {
                    return Some(layer);
                }
            }
        }
        None
    }

    /// Get a named metric from the default master, if present
    pub fn default_metric(&self, name: &str) -> Option<i32> {
        let metric: MetricType = MetricType::from(name);
        self.default_master()
            .and_then(|m| m.metrics.get(&metric))
            .copied()
    }

    pub(crate) fn fontdrasil_axes(&self) -> Result<fontdrasil::types::Axes, BabelfontError> {
        let axes: Result<Vec<fontdrasil::types::Axis>, _> =
            self.axes.iter().map(|ax| ax.clone().try_into()).collect();
        Ok(fontdrasil::types::Axes::new(axes?))
    }

    /// Normalizes a location between -1.0 and 1.0
    pub fn normalize_location<Space>(
        &self,
        loc: Location<Space>,
    ) -> Result<NormalizedLocation, BabelfontError>
    where
        Space: fontdrasil::coords::ConvertSpace<NormalizedSpace>,
    {
        loc.convert(&self.fontdrasil_axes()?).map_err(|e| e.into())
    }

    // other location conversion functions could go here
    // unicode map int -> glyph name
    // get variable anchor
    // exported glyphs

    // fn axis_order(&self) -> Vec<Tag> {
    //     self.axes.iter().map(|ax| ax.tag.clone()).collect()
    // }

    /// Save the font to a file
    ///
    /// Which file formats are supported will depend on which features are enabled:
    ///  - With no features, only the `.babelfont` JSON format is supported
    ///  - With the `ufo` feature, `.designspace` files and `.ufo` are also supported
    ///  - With the `glyphs` feature, `.glyphs` files are also supported
    ///  - With the `fontir` feature, `.ttf` files are also supported
    pub fn save<T: Into<std::path::PathBuf>>(&self, path: T) -> Result<(), BabelfontError> {
        let path = path.into();
        if path.extension().and_then(|x| x.to_str()) == Some("babelfont") {
            let file = std::fs::File::create(&path)?;
            let mut buffer = std::io::BufWriter::new(file);
            serde_json::to_writer_pretty(&mut buffer, &self)?;
            return Ok(());
        }

        #[cfg(feature = "fontir")]
        {
            if path.extension().and_then(|x| x.to_str()) == Some("ttf") {
                use crate::convertors::fontir::CompilationOptions;

                let bytes = crate::convertors::fontir::BabelfontIrSource::compile(
                    self.clone(),
                    CompilationOptions::default(),
                )?;
                std::fs::write(&path, bytes)?;
                return Ok(());
            }
        }

        #[cfg(feature = "glyphs")]
        {
            if path.extension().and_then(|x| x.to_str()) == Some("glyphs") {
                let glyphs3_font = self.as_glyphslib()?;
                return glyphs3_font
                    .save(&path)
                    .map_err(|x| BabelfontError::PlistParse(x.to_string()));
            }
        }
        #[cfg(feature = "ufo")]
        {
            if path.extension().and_then(|x| x.to_str()) == Some("designspace") {
                crate::convertors::designspace::save_designspace(self, &path)?;
            }
        }

        Err(BabelfontError::UnknownFileType {
            path: path.to_path_buf(),
        })
    }

    /// Interpolate a glyph at a given location in design space
    pub fn interpolate_glyph(
        &self,
        glyphname: &str,
        location: &Location<DesignSpace>,
    ) -> Result<crate::Layer, BabelfontError> {
        let glyph = self
            .glyphs
            .get(glyphname)
            .ok_or_else(|| BabelfontError::GlyphNotFound {
                glyph: glyphname.to_string(),
            })?;
        let axes = self.fontdrasil_axes()?;
        let target_location = location.to_normalized(&axes)?;

        let mut layers: Vec<(DesignLocation, &Layer)> = vec![];
        for layer in &glyph.layers {
            if let Some(master) = self
                .masters
                .iter()
                .find(|m| Some(&m.id) == layer.id.as_ref())
            {
                layers.push((master.location.clone(), layer));
            } else if let Some(loc) = &layer.location {
                // Intermediate layer
                layers.push((loc.clone(), layer));
            }
        }
        // Put default master first, if we can find it
        if let Some(default_master_index) = self.default_master_index() {
            let default_master = &self.masters[default_master_index];
            layers.sort_by_key(|(loc, _)| {
                if *loc == default_master.location {
                    0
                } else {
                    1
                }
            });
        }
        crate::interpolate::interpolate_layer(glyphname, &layers, &axes, &target_location)
    }
}

#[cfg(feature = "glyphs")]
mod glyphs {
    use crate::BabelfontError;

    use super::Font;

    impl Font {
        /// Convert to a glyphslib::Font in glyphs 3 format
        pub fn as_glyphslib(&self) -> Result<glyphslib::Font, BabelfontError> {
            Ok(glyphslib::Font::Glyphs3(
                crate::convertors::glyphs3::as_glyphs3(self)?,
            ))
        }
    }
}

#[cfg(feature = "fontra")]
mod fontra {
    use std::collections::HashMap;

    use fontdrasil::coords::DesignLocation;

    use super::Font;
    use crate::{convertors::fontra, BabelfontError};
    impl Font {
        /// Return a [fontra::FontInfo] representation of this font's naming and version data
        pub fn as_fontra_info(&self) -> fontra::FontInfo {
            fontra::FontInfo {
                family_name: self.names.family_name.get_default().cloned(),
                version_major: Some(self.version.0),
                version_minor: Some(self.version.1),
                copyright: self.names.copyright.get_default().cloned(),
                trademark: self.names.trademark.get_default().cloned(),
                description: self.names.description.get_default().cloned(),
                sample_text: self.names.sample_text.get_default().cloned(),
                designer: self.names.designer.get_default().cloned(),
                designer_url: self.names.designer_url.get_default().cloned(),
                manufacturer: self.names.manufacturer.get_default().cloned(),
                manufacturer_url: self.names.manufacturer_url.get_default().cloned(),
                license_description: self.names.license.get_default().cloned(),
                license_info_url: self.names.license_url.get_default().cloned(),
                vendor_id: None,
                custom_data: HashMap::new(),
            }
        }

        /// Return a [fontra::Axes] representation of this font's axes
        pub fn as_fontra_axes(&self) -> Result<fontra::Axes, BabelfontError> {
            Ok(fontra::Axes {
                axes: self
                    .axes
                    .iter()
                    .map(fontra::Axis::try_from)
                    .collect::<Result<Vec<_>, _>>()?,
                mappings: vec![],
                elided_fall_backname: "".to_string(),
            })
        }

        /// Get a [fontra::Glyph] representation of a glyph by name
        pub fn get_fontra_glyph(&self, glyphname: &str) -> Option<fontra::Glyph> {
            let our_glyph = self.glyphs.get(glyphname)?;
            let mut glyph = fontra::Glyph {
                name: our_glyph.name.to_string(),
                axes: vec![],
                sources: vec![],
                layers: HashMap::new(),
            };
            let master_locations: HashMap<String, &DesignLocation> = self
                .masters
                .iter()
                .map(|m| (m.id.clone(), &m.location))
                .collect::<HashMap<String, _>>();
            for layer in our_glyph.layers.iter() {
                let layer_id = layer.id.clone().unwrap_or("Unknown layer".to_string());
                glyph.layers.insert(layer_id.clone(), layer.into());
                glyph.sources.push(fontra::GlyphSource {
                    name: layer_id.clone(),
                    layer_name: layer_id.clone(),
                    location: master_locations
                        .get(&layer_id.clone())
                        .map(|loc| {
                            loc.iter()
                                .map(|(k, v)| (k.to_string(), v.to_f64()))
                                .collect::<HashMap<String, f64>>()
                        })
                        .unwrap_or_default(),
                })
            }
            Some(glyph)
        }
    }
}
