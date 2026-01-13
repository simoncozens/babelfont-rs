use std::path::{self, PathBuf};

use fontdrasil::{
    coords::{DesignCoord, Location, UserCoord},
    types::Tag,
};
use uuid::Uuid;
use vfbreader::{read_vfb, GlyphEntry, Node as VFBNode, Vfb, VfbEntry};

use crate::{Axis, BabelfontError, Font, Glyph, Layer, LayerType, Master, OutlinePen as _, Shape};
/// VFB convertor
pub fn load(path: PathBuf) -> Result<Font, BabelfontError> {
    let vfb: Vfb = read_vfb(&path).map_err(|e| BabelfontError::VfbLoad(e.into()))?;

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
            VfbEntry::EncodingDefault(encoding) => {} // => todo!(),
            VfbEntry::Encoding(encoding) => {}        // => todo!(),
            VfbEntry::Unknown1502(_) => {}            // => todo!(),
            VfbEntry::Unknown518 => {}                // => todo!(),
            VfbEntry::Unknown257(_) => {}             // => todo!(),
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
            VfbEntry::Version(_) => {} // => todo!(),
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
            VfbEntry::IsFixedPitch(_) => {}       // => todo!(),
            VfbEntry::UnderlineThickness(_) => {} // => todo!(),
            VfbEntry::MsCharset(_) => {}          // => todo!(),
            VfbEntry::Panose(_) => {}             // => todo!(),
            VfbEntry::TtVersion(_) => {}          // => todo!(),
            VfbEntry::TtUId(_) => {}              // => todo!(),
            VfbEntry::StyleName(_) => {}          // => todo!(),
            VfbEntry::PrefStyleName(_) => {}      // => todo!(),
            VfbEntry::MacCompatible(_) => {}      // => todo!(),
            VfbEntry::Vendor(_) => {}             // => todo!(),
            VfbEntry::Year(_) => {}               // => todo!(),
            VfbEntry::VersionMajor(_) => {}       // => todo!(),
            VfbEntry::VersionMinor(_) => {}       // => todo!(),
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
            VfbEntry::Unknown1604(_) => {}         // => todo!(),
            VfbEntry::Unknown2032(_) => {}         // => todo!(),
            VfbEntry::ExportPcltTable(_) => {}     // => todo!(),
            VfbEntry::Note(_) => {}                // => todo!(),
            VfbEntry::CustomData(_) => {}          // => todo!(),
            VfbEntry::OpenTypeClass(_) => {}       // => todo!(),
            VfbEntry::AxisCount(_) => {}           // => todo!(),
            VfbEntry::AxisName(_) => {}            // => todo!(),
            VfbEntry::MasterName(_) => {}          // => todo!(),
            VfbEntry::DefaultCharacter(_) => {}    // => todo!(),
            VfbEntry::Unknown2034(_) => {}         // => todo!(),
            VfbEntry::Mark(_) => {}                // => todo!(),
            VfbEntry::GlyphCustomData(_) => {}     // => todo!(),
            VfbEntry::GlyphNote(_) => {}           // => todo!(),
            VfbEntry::WeightVector(items) => {}    // => todo!(),
            VfbEntry::UniqueId(_) => {}            // => todo!(),
            VfbEntry::WeightCode(_) => {}          // => todo!(),
            VfbEntry::ItalicAngle(_) => {}         // => todo!(),
            VfbEntry::SlantAngle(_) => {}          // => todo!(),
            VfbEntry::UnderlinePosition(_) => {}   // => todo!(),
            VfbEntry::E1140(raw_data) => {}        // => todo!(),
            VfbEntry::Xuid(items) => {}            // => todo!(),
            VfbEntry::XuidNum(_) => {}             // => todo!(),
            VfbEntry::PostScriptHintingOptions(post_script_global_hinting_options) => {} // => todo!(),
            VfbEntry::E1068(items) => {}                // => todo!(),
            VfbEntry::TtInfo(true_type_values) => {}    // => todo!(),
            VfbEntry::UnicodeRanges(items) => {}        // => todo!(),
            VfbEntry::FontNames(name_records) => {}     // => todo!(),
            VfbEntry::CustomCmaps(raw_data) => {}       // => todo!(),
            VfbEntry::PcltTable(raw_data) => {}         // => todo!(),
            VfbEntry::E2030(raw_data) => {}             // => todo!(),
            VfbEntry::MetricsClassFlags(raw_data) => {} // => todo!(),
            VfbEntry::KerningClassFlags(raw_data) => {} // => todo!(),
            VfbEntry::TrueTypeTable(binary_table) => {} // => todo!(),
            VfbEntry::Features(_) => {}                 // => todo!(),
            VfbEntry::E513(raw_data) => {}              // => todo!(),
            VfbEntry::E271(raw_data) => {}              // => todo!(),
            VfbEntry::AnisotropicInterpolationMappings(raw_data) => {} // => todo!(),
            VfbEntry::AxisMappingsCount(_) => {}        // => todo!(),
            VfbEntry::AxisMappings(raw_data) => {}      // => todo!(),
            VfbEntry::PrimaryInstanceLocations(items) => {} // => todo!(),
            VfbEntry::PrimaryInstances(raw_data) => {}  // => todo!(),
            VfbEntry::E527(raw_data) => {}              // => todo!(),
            VfbEntry::GlobalGuides(guides) => {}        // => todo!(),
            VfbEntry::GlobalGuideProperties(raw_data) => {} // => todo!(),
            VfbEntry::GlobalMask(raw_data) => {}        // => todo!(),
            VfbEntry::OpenTypeExportOptions(raw_data) => {} // => todo!(),
            VfbEntry::ExportOptions(export_options) => {} // => todo!(),
            VfbEntry::MappingMode(raw_data) => {}       // => todo!(),
            VfbEntry::E272(raw_data) => {}              // => todo!(),
            VfbEntry::E1410(raw_data) => {}             // => todo!(),
            VfbEntry::E528(raw_data) => {}              // => todo!(),
            VfbEntry::MasterLocation(raw_data) => {}    // => todo!(),
            VfbEntry::PostScriptInfo(raw_data) => {}    // => todo!(),
            VfbEntry::Cvt(raw_data) => {}               // => todo!(),
            VfbEntry::Prep(raw_data) => {}              // => todo!(),
            VfbEntry::Fpgm(raw_data) => {}              // => todo!(),
            VfbEntry::Gasp(raw_data) => {}              // => todo!(),
            VfbEntry::Vdmx(raw_data) => {}              // => todo!(),
            VfbEntry::HheaAscender(_) => {}             // => todo!(),
            VfbEntry::HheaDescender(_) => {}            // => todo!(),
            VfbEntry::TrueTypeStemPpems2And3(raw_data) => {} // => todo!(),
            VfbEntry::TrueTypeStemPpems(raw_data) => {} // => todo!(),
            VfbEntry::TrueTypeStems(raw_data) => {}     // => todo!(),
            VfbEntry::TrueTypeStemPpems1(raw_data) => {} // => todo!(),
            VfbEntry::TrueTypeZones(raw_data) => {}     // => todo!(),
            VfbEntry::TrueTypeZoneDeltas(raw_data) => {} // => todo!(),
            VfbEntry::Glyph(items) => load_glyph(&mut font, items)?,
            VfbEntry::Links(links) => {}      // => todo!(),
            VfbEntry::Image(raw_data) => {}   // => todo!(),
            VfbEntry::Bitmaps(raw_data) => {} // => todo!(),
            VfbEntry::E2023(raw_data) => {}   // => todo!(),
            VfbEntry::Sketch(raw_data) => {}  // => todo!(),
            VfbEntry::HintingOptions(post_script_glyph_hinting_options) => {} // => todo!(),
            VfbEntry::Mask(raw_data) => {}    // => todo!(),
            VfbEntry::MaskMetrics(raw_data) => {} // => todo!(),
            VfbEntry::MaskMetricsMm(raw_data) => {} // => todo!(),
            VfbEntry::Origin(_) => {}         // => todo!(),
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
            VfbEntry::GdefData(raw_data) => {} // => todo!(),
            VfbEntry::AnchorsProperties(anchors_supplementals) => {} // => todo!(),
            VfbEntry::AnchorsMm(items) => {}   // => todo!(),
            VfbEntry::GuideProperties(raw_data) => {} // => todo!(),
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
            GlyphEntry::Hints(hints) => {}           // => todo!(),
            GlyphEntry::Guides(guides) => {}         // => todo!(),
            GlyphEntry::Components(components) => {} // => todo!(),
            GlyphEntry::Kerning(hash_map) => {}      // => todo!(),
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
            GlyphEntry::Binary(raw_data) => {}       // => todo!(),
            GlyphEntry::Instructions(raw_data) => {} // => todo!(),
        }
    }
    font.glyphs.push(glyph);
    Ok(())
}
