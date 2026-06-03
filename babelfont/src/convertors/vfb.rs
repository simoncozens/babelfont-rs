use std::path::PathBuf;

use fontdrasil::{
    coords::{DesignCoord, Location, UserCoord},
    types::Tag,
};
use indexmap::IndexMap;
use uuid::Uuid;
use vfbreader::{read_vfb, GlyphEntry, Node as VFBNode, Vfb, VfbEntry};

use crate::{
    common::decomposition::DecomposedAffine, features::PossiblyAutomaticCode, Axis, BabelfontError,
    Features, Font, FormatSpecific, Glyph, Layer, LayerType, Master, OutlinePen as _, Shape,
};
/// VFB convertor
pub fn load(path: PathBuf) -> Result<Font, BabelfontError> {
    let vfb: Vfb = read_vfb(&path).map_err(|e| BabelfontError::VfbLoad(e.to_string()))?;

    // Convert Vfb to Babelfont Font
    let mut font = Font::new();
    let mut wght = Axis::new("Weight", Tag::from_be_bytes(*b"wght"));
    wght.min = Some(UserCoord::new(100.0));
    wght.max = Some(UserCoord::new(900.0));
    wght.default = Some(UserCoord::new(400.0));
    font.axes = vec![wght];
    // Create a default master
    font.masters = vec![Master::new(
        "Default Master",
        Uuid::new_v4().to_string(),
        vec![(Tag::from_be_bytes(*b"wght"), DesignCoord::new(400.0))]
            .into_iter()
            .collect(),
    )];
    for entry in vfb.entries {
        match entry {
            VfbEntry::EncodingDefault(_encoding) => {} // => todo!(),
            VfbEntry::Encoding(_encoding) => {}        // => todo!(),
            VfbEntry::MMEncodingType(_) => {}          // => todo!(),
            VfbEntry::BlockNamesEnd(_) => {}
            VfbEntry::BlockFontInfoStart(_) => {}
            VfbEntry::FontName(s) => {
                font.names.family_name = s.into();
            }
            VfbEntry::MasterCount(masters) => {
                // Make some more
                for i in 1..masters {
                    font.masters.push(Master::new(
                        format!("Master {}", i + 1),
                        Uuid::new_v4().to_string(),
                        Location::default(),
                    ));
                }
            }
            VfbEntry::Version(v) => {
                font.names.version = v.into();
            }
            VfbEntry::Notice(s) => font.names.description = s.into(),
            VfbEntry::FullName(s) => font.names.full_name = s.into(),
            VfbEntry::FamilyName(_) => {} // => todo!(),
            VfbEntry::PrefFamilyName(s) => font.names.preferred_subfamily_name = s.into(),
            VfbEntry::MenuName(_) => {}  // => todo!(),
            VfbEntry::AppleName(_) => {} // => todo!(),
            VfbEntry::Weight(_) => {}    // => todo!(),
            VfbEntry::Width(_) => {}     // => todo!(),
            VfbEntry::License(s) => font.names.license = s.into(),
            VfbEntry::LicenseUrl(s) => font.names.license_url = s.into(),
            VfbEntry::Copyright(s) => font.names.copyright = s.into(),
            VfbEntry::Trademark(serde) => font.names.trademark = serde.into(),
            VfbEntry::Designer(s) => font.names.designer = s.into(),
            VfbEntry::DesignerUrl(s) => font.names.designer_url = s.into(),
            VfbEntry::VendorUrl(s) => font.names.manufacturer_url = s.into(),
            VfbEntry::Source(s) => font.names.manufacturer = s.into(),
            VfbEntry::IsFixedPitch(_) => {} // => todo!(),
            VfbEntry::UnderlineThickness(ut) => {
                font.masters[0]
                    .metrics
                    .insert(crate::MetricType::UnderlineThickness, ut as i32);
            }
            VfbEntry::MsCharset(_) => {}     // => todo!(),
            VfbEntry::Panose(_) => {}        // => todo!(),
            VfbEntry::TtVersion(_) => {}     // => todo!(),
            VfbEntry::TtUId(_) => {}         // => todo!(),
            VfbEntry::StyleName(_) => {}     // => todo!(),
            VfbEntry::PrefStyleName(_) => {} // => todo!(),
            VfbEntry::MacCompatible(_) => {} // => todo!(),
            VfbEntry::Vendor(_) => {}        // => todo!(),
            VfbEntry::Year(_) => {}          // => todo!(),
            VfbEntry::VersionMajor(v) => {
                font.version = (v, font.version.1);
            }
            VfbEntry::VersionMinor(v) => {
                font.version = (font.version.0, v);
            }
            VfbEntry::Upm(u) => {
                font.upm = u;
            }
            VfbEntry::FondId(_) => {}              // => todo!(),
            VfbEntry::BlueValuesNum(_) => {}       // => todo!(),
            VfbEntry::OtherBluesNum(_) => {}       // => todo!(),
            VfbEntry::FamilyBluesNum(_) => {}      // => todo!(),
            VfbEntry::FamilyOtherBluesNum(_) => {} // => todo!(),
            VfbEntry::StemSnapHNum(_) => {}        // => todo!(),
            VfbEntry::StemSnapVNum(_) => {}        // => todo!(),
            VfbEntry::FontStyle(_) => {}           // => todo!(),
            VfbEntry::PclId(_) => {}               // => todo!(),
            VfbEntry::VpId(_) => {}                // => todo!(),
            VfbEntry::MsId(_) => {}                // => todo!(),
            VfbEntry::PclCharsSet(_) => {}         // => todo!(),
            VfbEntry::HheaLineGap(_) => {}         // => todo!(),
            VfbEntry::StemSnapLimit(_) => {}       // => todo!(),
            VfbEntry::ZonePpm(_) => {}             // => todo!(),
            VfbEntry::CodePpm(_) => {}             // => todo!(),
            VfbEntry::DropoutPpm(_) => {}          // => todo!(),
            VfbEntry::MeasurementLine(_) => {}     // => todo!(),
            VfbEntry::ExportPcltTable(_) => {}     // => todo!(),
            VfbEntry::Note(_) => {}                // => todo!(),
            VfbEntry::CustomData(_) => {}          // => todo!(),
            VfbEntry::OpenTypeClass(s) => {
                let splits = s.splitn(2, ":").collect::<Vec<_>>();
                if splits.len() == 2 {
                    let (classname, contents) = (splits[0], splits[1]);
                    // Not sure what this is for,
                    let contents = contents.replace("'", "");
                    font.features
                        .classes
                        .insert(classname.into(), PossiblyAutomaticCode::new(contents));
                }
            }
            VfbEntry::AxisCount(_) => {}         // => todo!(),
            VfbEntry::AxisName(_) => {}          // => todo!(),
            VfbEntry::MasterName(_) => {}        // => todo!(),
            VfbEntry::DefaultCharacter(_) => {}  // => todo!(),
            VfbEntry::CustomDict(_) => {}        // => todo!(),
            VfbEntry::Mark(_) => {}              // => todo!(),
            VfbEntry::GlyphCustomData(_) => {}   // => todo!(),
            VfbEntry::GlyphNote(_) => {}         // => todo!(),
            VfbEntry::WeightVector(_items) => {} // => todo!(),
            VfbEntry::UniqueId(_) => {}          // => todo!(),
            VfbEntry::WeightCode(_) => {}        // => todo!(),
            VfbEntry::ItalicAngle(angle) => {
                // XXX angle conversion?
                font.masters[0]
                    .metrics
                    .insert(crate::MetricType::ItalicAngle, angle as i32);
            } // => todo!(),
            VfbEntry::SlantAngle(_) => {}        // => todo!(),
            VfbEntry::UnderlinePosition(_) => {} // => todo!(),
            VfbEntry::SampleText(s) => {
                font.names.sample_text = s.into();
            }
            VfbEntry::Xuid(_items) => {} // => todo!(),
            VfbEntry::XuidNum(_) => {}   // => todo!(),
            VfbEntry::PostScriptHintingOptions(_post_script_global_hinting_options) => {} // => todo!(),
            VfbEntry::Collection(_items) => {} // => todo!(_),
            VfbEntry::TtInfo(_true_type_values) => {} // _=> todo!(),
            VfbEntry::UnicodeRanges(_items) => {} // => todo!(),
            VfbEntry::FontNames(_name_records) => {} // => todo!(),
            VfbEntry::CustomCmaps(_raw_data) => {} // => todo!(),
            VfbEntry::PcltTable(_raw_data) => {} // => todo!(),
            VfbEntry::FontFlags(_raw_data) => {} // => todo!(),
            VfbEntry::MetricsClassFlags(_raw_data) => {} // => todo!(),
            VfbEntry::KerningClassFlags(_raw_data) => {} // => todo!(),
            VfbEntry::TrueTypeTable(_binary_table) => {} // => todo!(),
            VfbEntry::Features(code) => {
                let features = Features::from_fea(&code);
                font.features.features = features.features;
                font.features.prefixes = features.prefixes;
            }
            VfbEntry::BlockFontInfoEnd(_raw_data) => {}
            VfbEntry::BlockMMFontInfoStart(_raw_data) => {}
            VfbEntry::AnisotropicInterpolationMappings(_raw_data) => {} // => todo!(),
            VfbEntry::AxisMappingsCount(_) => {}                        // => todo!(),
            VfbEntry::AxisMappings(_raw_data) => {}                     // => todo!(),
            VfbEntry::PrimaryInstanceLocations(_items) => {}            // => todo!(),
            VfbEntry::PrimaryInstances(_raw_data) => {}                 // => todo!(),
            VfbEntry::BlockMMFontInfoEnd(_raw_data) => {}
            VfbEntry::GlobalGuides(_guides) => {} // => todo!(),
            VfbEntry::GlobalGuideProperties(_raw_data) => {} // => todo!(),
            VfbEntry::GlobalMask(_raw_data) => {} // => todo!(),
            VfbEntry::OpenTypeExportOptions(_raw_data) => {} // => todo!(),
            VfbEntry::ExportOptions(_export_options) => {} // => todo!(),
            VfbEntry::MappingMode(_raw_data) => {} // => todo!(),
            VfbEntry::BlockMMKerningStart(_raw_data) => {}
            VfbEntry::MMKernPair(_raw_data) => {} // => todo!(),
            VfbEntry::BlockMMKerningEnd(_raw_data) => {}
            VfbEntry::MasterLocation(_raw_data) => {} // => todo!(),
            VfbEntry::PostScriptInfo(_raw_data) => {} // => todo!(),
            VfbEntry::Cvt(_raw_data) => {}            // => todo!(),
            VfbEntry::Prep(_raw_data) => {}           // => todo!(),
            VfbEntry::Fpgm(_raw_data) => {}           // => todo!(),
            VfbEntry::Gasp(_raw_data) => {}           // => todo!(),
            VfbEntry::Vdmx(_raw_data) => {}           // => todo!(),
            VfbEntry::HheaAscender(asc) => {
                font.masters[0]
                    .metrics
                    .insert(crate::MetricType::Ascender, asc.into());
                font.masters[0]
                    .metrics
                    .insert(crate::MetricType::HheaAscender, asc.into());
            }

            VfbEntry::HheaDescender(desc) => {
                font.masters[0]
                    .metrics
                    .insert(crate::MetricType::Descender, desc.into());
                font.masters[0]
                    .metrics
                    .insert(crate::MetricType::HheaDescender, desc.into());
            }
            VfbEntry::TrueTypeStemPpems2And3(_raw_data) => {} // => todo!(),
            VfbEntry::TrueTypeStemPpems(_raw_data) => {}      // => todo!(),
            VfbEntry::TrueTypeStems(_raw_data) => {}          // => todo!(),
            VfbEntry::TrueTypeStemPpems1(_raw_data) => {}     // => todo!(),
            VfbEntry::TrueTypeZones(_raw_data) => {}          // => todo!(),
            VfbEntry::TrueTypeZoneDeltas(_raw_data) => {}     // => todo!(),
            VfbEntry::Glyph(items) => load_glyph(&mut font, items)?,
            VfbEntry::Links(_links) => {}      // => todo!(),
            VfbEntry::Image(_raw_data) => {}   // => todo!(),
            VfbEntry::Bitmaps(_raw_data) => {} // => todo!(),
            VfbEntry::VSB(_raw_data) => {}     // => todo!(),
            VfbEntry::Sketch(_raw_data) => {}  // => todo!(),
            VfbEntry::HintingOptions(_post_script_glyph_hinting_options) => {} // => todo!(),
            VfbEntry::Mask(_raw_data) => {}    // => todo!(),
            VfbEntry::MaskMetrics(_raw_data) => {} // => todo!(),
            VfbEntry::MaskMetricsMm(_raw_data) => {} // => todo!(),
            VfbEntry::Origin(_) => {}          // => todo!(),
            VfbEntry::Unicodes(codepoints) => {
                // Set codepoints for last glyph
                if let Some(glyph) = font.glyphs.last_mut() {
                    glyph.codepoints = codepoints.iter().map(|&c| c as u32).collect();
                }
            }
            VfbEntry::UnicodesNonBmp(items) => {
                // Extend the codepoints
                if let Some(glyph) = font.glyphs.last_mut() {
                    glyph.codepoints.extend(items.iter().copied());
                }
            } // => todo!(),
            VfbEntry::GdefData(_raw_data) => {} // => todo!(),
            VfbEntry::AnchorsProperties(_anchors_supplementals) => {} // => todo!(),
            VfbEntry::AnchorsMm(_items) => {}   // => todo!(),
            VfbEntry::GuideProperties(_raw_data) => {} // => todo!(),
        }
    }

    // Fix up component ID->name references
    let names = font
        .glyphs
        .iter()
        .map(|g| g.name.clone())
        .collect::<Vec<_>>();
    for glyph in font.glyphs.iter_mut() {
        for layer in glyph.layers.iter_mut() {
            for shape in layer.shapes.iter_mut() {
                if let Shape::Component(component) = shape {
                    #[allow(clippy::unwrap_used)] // We put it there
                    let ref_id: usize = component.reference.parse().unwrap();
                    if let Some(ref_glyph) = names.get(ref_id) {
                        component.reference = ref_glyph.clone();
                    }
                }
            }
        }
    }
    Ok(font)
}

fn load_glyph(font: &mut Font, items: Vec<GlyphEntry>) -> Result<(), BabelfontError> {
    let mut glyph = Glyph::new("unnamed");
    glyph.exported = true;
    // Create a layer for each master
    for i in 0..font.masters.len() {
        let mut layer = Layer::new(0.0); // will fix up with metrics
        layer.master = LayerType::DefaultForMaster(font.masters[i].id.clone());
        layer.id = Some(font.masters[i].id.clone());
        glyph.layers.push(layer);
    }

    for item in items {
        match item {
            GlyphEntry::GlyphName(s) => {
                glyph.name = s.into();
            }
            GlyphEntry::Metrics(items) => {
                for (i, (width, _height)) in items.into_iter().enumerate() {
                    if let Some(layer) = glyph.layers.get_mut(i) {
                        layer.width = width as f32;
                    }
                }
            }
            GlyphEntry::Hints(_hints) => {}   // => todo!(),
            GlyphEntry::Guides(_guides) => {} // => todo!(),
            GlyphEntry::Components(components) => {
                for (index, layer) in glyph.layers.iter_mut().enumerate() {
                    for component in components.iter() {
                        let our_component = crate::shape::Component {
                            reference: component.glyph_index.to_string().into(), // Will fix later
                            transform: DecomposedAffine {
                                translation: (
                                    *component.x_offset.get(index).ok_or_else(|| {
                                        BabelfontError::GlyphNotInterpolatable {
                                            glyph: glyph.name.clone().into(),
                                            reason: "Not enough coordinates for component"
                                                .to_string(),
                                        }
                                    })? as f64,
                                    *component.y_offset.get(index).ok_or_else(|| {
                                        BabelfontError::GlyphNotInterpolatable {
                                            glyph: glyph.name.clone().into(),
                                            reason: "Not enough coordinates for component"
                                                .to_string(),
                                        }
                                    })? as f64,
                                ),
                                scale: (
                                    *component.x_scale.get(index).ok_or_else(|| {
                                        BabelfontError::GlyphNotInterpolatable {
                                            glyph: glyph.name.clone().into(),
                                            reason: "Not enough coordinates for component"
                                                .to_string(),
                                        }
                                    })?,
                                    *component.y_scale.get(index).ok_or_else(|| {
                                        BabelfontError::GlyphNotInterpolatable {
                                            glyph: glyph.name.clone().into(),
                                            reason: "Not enough coordinates for component"
                                                .to_string(),
                                        }
                                    })?,
                                ),
                                ..Default::default()
                            },
                            location: IndexMap::new(),
                            format_specific: FormatSpecific::default(),
                        };
                        layer.shapes.push(Shape::Component(our_component));
                    }
                }
            } // => todo!(),
            GlyphEntry::Kerning(_hash_map) => {} // => todo!(),
            GlyphEntry::Outlines(nodes) => {
                // For each layer, build the path
                for (index, layer) in glyph.layers.iter_mut().enumerate() {
                    let mut pathbuilder = crate::shape::PathBuilder::new();
                    for node in nodes.iter() {
                        match node {
                            VFBNode::Move { coords, flags: _ } => {
                                // close any open path
                                pathbuilder.close();
                                let these_coords = coords.get(index).ok_or_else(|| {
                                    BabelfontError::GlyphNotInterpolatable {
                                        glyph: glyph.name.clone().into(),
                                        reason: "Not enough coordinates for move node".to_string(),
                                    }
                                })?;
                                pathbuilder.move_to(these_coords.0 as f32, these_coords.1 as f32);
                            }
                            VFBNode::Line { coords, flags: _ } => {
                                let these_coords = coords.get(index).ok_or_else(|| {
                                    BabelfontError::GlyphNotInterpolatable {
                                        glyph: glyph.name.clone().into(),
                                        reason: "Not enough coordinates for line node".to_string(),
                                    }
                                })?;
                                pathbuilder.line_to(these_coords.0 as f32, these_coords.1 as f32);
                            }
                            VFBNode::Curve {
                                coords,
                                c1_coords,
                                c2_coords,
                                flags: _,
                            } => {
                                let these_coords = coords.get(index).ok_or_else(|| {
                                    BabelfontError::GlyphNotInterpolatable {
                                        glyph: glyph.name.clone().into(),
                                        reason: "Not enough coordinates for curve node".to_string(),
                                    }
                                })?;
                                let these_c1_coords = c1_coords.get(index).ok_or_else(|| {
                                    BabelfontError::GlyphNotInterpolatable {
                                        glyph: glyph.name.clone().into(),
                                        reason: "Not enough c1 coordinates for curve node"
                                            .to_string(),
                                    }
                                })?;
                                let these_c2_coords = c2_coords.get(index).ok_or_else(|| {
                                    BabelfontError::GlyphNotInterpolatable {
                                        glyph: glyph.name.clone().into(),
                                        reason: "Not enough c2 coordinates for curve node"
                                            .to_string(),
                                    }
                                })?;
                                pathbuilder.curve_to(
                                    these_c1_coords.0 as f32,
                                    these_c1_coords.1 as f32,
                                    these_c2_coords.0 as f32,
                                    these_c2_coords.1 as f32,
                                    these_coords.0 as f32,
                                    these_coords.1 as f32,
                                );
                            }
                            VFBNode::QCurve {
                                coords,
                                c1_coords,
                                flags: _,
                            } => {
                                let these_coords = coords.get(index).ok_or_else(|| {
                                    BabelfontError::GlyphNotInterpolatable {
                                        glyph: glyph.name.clone().into(),
                                        reason: "Not enough coordinates for qcurve node"
                                            .to_string(),
                                    }
                                })?;
                                let these_c1_coords = c1_coords.get(index).ok_or_else(|| {
                                    BabelfontError::GlyphNotInterpolatable {
                                        glyph: glyph.name.clone().into(),
                                        reason: "Not enough c1 coordinates for qcurve node"
                                            .to_string(),
                                    }
                                })?;
                                pathbuilder.quad_to(
                                    these_c1_coords.0 as f32,
                                    these_c1_coords.1 as f32,
                                    these_coords.0 as f32,
                                    these_coords.1 as f32,
                                );
                            }
                        }
                    }
                    pathbuilder.close();
                    let paths = pathbuilder.build();
                    layer.shapes.extend(paths.into_iter().map(Shape::Path));
                }
            }
            GlyphEntry::Binary(_raw_data) => {} // => todo!(),
            GlyphEntry::Instructions(_raw_data) => {} // => todo!(),
        }
    }
    font.glyphs.push(glyph);
    Ok(())
}
