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
    Features, Font, FormatSpecific, Glyph, Layer, LayerType, Master, Node, NodeType, Path, Shape,
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
            VfbEntry::FlVersion(_) => {}               // => todo!(),
            VfbEntry::FontOptions(_) => {}             // => todo!(),
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
            // VfbEntry::OpenTypeExportOptions(_raw_data) => {} // => todo!(),
            VfbEntry::ExportOptions(_export_options) => {} // => todo!(),
            VfbEntry::MappingMode(_raw_data) => {}         // => todo!(),
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
            VfbEntry::BlockFileDataStart(_) => {} // todo!(),
            VfbEntry::BlockFontStart(_) => {}   // todo!(),
            VfbEntry::BlockNamesStart(_) => {}  // todo!(),
            VfbEntry::BlockFontEnd(_) | VfbEntry::BlockFileDataEnd(_) => {}
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
                // For each layer (i.e. master), build the paths
                for (index, layer) in glyph.layers.iter_mut().enumerate() {
                    let paths = outline_to_paths(&nodes, index, glyph.name.as_str())?;
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

/// Fetch one master's coordinates from a VFB node, converting the error when
/// the node does not have a point for that master.
fn master_coords(
    coords: &[(i32, i32)],
    index: usize,
    what: &str,
    glyph_name: &str,
) -> Result<(f64, f64), BabelfontError> {
    let (x, y) =
        coords
            .get(index)
            .copied()
            .ok_or_else(|| BabelfontError::GlyphNotInterpolatable {
                glyph: glyph_name.to_string(),
                reason: format!("Not enough coordinates for {what} node"),
            })?;
    Ok((x as f64, y as f64))
}

/// Convert one master's worth of VFB outline nodes into paths.
///
/// VFB stores quadratic curves differently from every other format we support.
/// Rather than pairing each off-curve control point with its own on-curve
/// endpoint, it emits a run of `qcurve` nodes (the off-curve control points)
/// terminated by a `line` node which is the on-curve point ending the run. (A
/// `line` node which is *not* preceded by `qcurve` nodes is a real straight
/// line.) We rebuild the conventional representation here, i.e. a run of
/// off-curve nodes ended by a single on-curve `QCurve` node.
fn outline_to_paths(
    nodes: &[VFBNode],
    master_index: usize,
    glyph_name: &str,
) -> Result<Vec<Path>, BabelfontError> {
    let mut paths = Vec::new();
    // The nodes of the contour we are currently building, in UFO point-pen
    // order.
    let mut contour: Vec<Node> = Vec::new();
    // Whether the node we last saw was a `qcurve` control point.
    let mut in_qcurve = false;
    // Whether the current contour is open (flag bit 3) rather than closed.
    let mut is_open = false;

    for node in nodes {
        match node {
            VFBNode::Move { coords, flags } => {
                if !contour.is_empty() {
                    finish_contour(&mut contour, is_open, &mut paths);
                }
                let (x, y) = master_coords(coords, master_index, "move", glyph_name)?;
                is_open = flags & 8 != 0;
                in_qcurve = false;
                let mut node = Node::new_move(x, y);
                node.smooth = flags & 1 != 0;
                contour.push(node);
            }
            VFBNode::Line { coords, flags } => {
                let (x, y) = master_coords(coords, master_index, "line", glyph_name)?;
                if in_qcurve {
                    // A line following qcurve control points is the on-curve
                    // point which ends the quadratic run.
                    let mut node = Node::new_qcurve(x, y);
                    node.smooth = flags & 1 != 0;
                    contour.push(node);
                    in_qcurve = false;
                } else {
                    let mut node = Node::new_line(x, y);
                    node.smooth = flags & 1 != 0;
                    contour.push(node);
                }
            }
            VFBNode::QCurve { coords, .. } => {
                // A qcurve node is an off-curve control point, not the end of
                // the segment.
                let (x, y) = master_coords(coords, master_index, "qcurve", glyph_name)?;
                contour.push(Node::new_offcurve(x, y));
                in_qcurve = true;
            }
            VFBNode::Curve {
                coords,
                c1_coords,
                c2_coords,
                flags,
            } => {
                let (c1x, c1y) = master_coords(c1_coords, master_index, "c1", glyph_name)?;
                let (c2x, c2y) = master_coords(c2_coords, master_index, "c2", glyph_name)?;
                let (x, y) = master_coords(coords, master_index, "curve", glyph_name)?;
                contour.push(Node::new_offcurve(c1x, c1y));
                contour.push(Node::new_offcurve(c2x, c2y));
                let mut node = Node::new_curve(x, y);
                node.smooth = flags & 1 != 0;
                contour.push(node);
                in_qcurve = false;
            }
        }
    }
    if !contour.is_empty() {
        finish_contour(&mut contour, is_open, &mut paths);
    }
    Ok(paths)
}

/// Finish the contour currently under construction, applying the same
/// closepath fixups that vfbLib performs.
fn finish_contour(contour: &mut Vec<Node>, is_open: bool, paths: &mut Vec<Path>) {
    if contour.is_empty() {
        return;
    }

    if !is_open {
        let last_index = contour.len() - 1;
        let last_nodetype = contour[last_index].nodetype;
        if last_nodetype == NodeType::OffCurve {
            // Trailing control points wrap around to the contour's start point,
            // which is therefore the on-curve node ending a quadratic run.
            contour[0].nodetype = NodeType::QCurve;
        } else {
            let closes_on_start =
                contour[0].x == contour[last_index].x && contour[0].y == contour[last_index].y;
            if contour.len() > 1
                && closes_on_start
                && !matches!(last_nodetype, NodeType::Line | NodeType::QCurve)
            {
                // The last node coincides with the start point: drop the
                // implicit move and rotate the last node to the front.
                #[allow(clippy::unwrap_used)] // The contour is non-empty
                let last = contour.pop().unwrap();
                contour[0] = last;
            } else {
                // Otherwise the closing segment is a straight line back to the
                // start point.
                contour[0].nodetype = NodeType::Line;
            }
        }
    }

    // Babelfont stores closed contours rotated one place from the UFO point
    // order (see `convertors::ufo::load_path`).
    let mut nodes = std::mem::take(contour);
    nodes.rotate_left(1);
    paths.push(Path {
        nodes,
        closed: !is_open,
        ..Default::default()
    });
}
