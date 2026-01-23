use crate::{convertors::fontir::CompilationOptions, Component, Font, Layer, NodeType, Shape};
use fontdrasil::{
    coords::{Location, NormalizedCoord, NormalizedLocation},
    orchestration::{Access, AccessBuilder, Work},
    types::GlyphName,
};
use fontir::{
    error::{BadGlyph, BadGlyphKind, Error, PathConversionError},
    ir::{self, AnchorBuilder, GlyphInstance, GlyphPathBuilder},
    orchestration::{Context, WorkId},
};
use kurbo::BezPath;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use write_fonts::types::Tag;

#[derive(Debug)]
pub(crate) struct GlyphIrWork {
    pub glyph_name: GlyphName,
    pub font: Arc<Font>,
    pub options: CompilationOptions,
}

fn check_pos(
    glyph_name: &GlyphName,
    positions: &HashSet<NormalizedCoord>,
    axis: &fontdrasil::types::Axis,
    pos: &NormalizedCoord,
) -> Result<(), BadGlyph> {
    if !positions.contains(pos) {
        return Err(BadGlyph::new(
            glyph_name.clone(),
            BadGlyphKind::UndefinedAtNormalizedPosition {
                axis: axis.tag,
                pos: pos.to_owned(),
            },
        ));
    }
    Ok(())
}

impl Work<Context, WorkId, Error> for GlyphIrWork {
    fn id(&self) -> WorkId {
        WorkId::Glyph(self.glyph_name.clone())
    }

    fn read_access(&self) -> Access<WorkId> {
        AccessBuilder::new()
            .variant(WorkId::StaticMetadata)
            .variant(WorkId::GlobalMetrics)
            .build()
    }

    fn write_access(&self) -> Access<WorkId> {
        AccessBuilder::new()
            .specific_instance(WorkId::Glyph(self.glyph_name.clone()))
            .specific_instance(WorkId::Anchor(self.glyph_name.clone()))
            .build()
    }

    fn also_completes(&self) -> Vec<WorkId> {
        vec![WorkId::Anchor(self.glyph_name.clone())]
    }

    fn exec(&self, context: &Context) -> Result<(), Error> {
        log::trace!("Generate IR for '{}'", self.glyph_name.as_str());
        let static_metadata = context.static_metadata.get();
        let axes = &static_metadata.all_source_axes;
        // let global_metrics = context.global_metrics.get();

        let glyph = self
            .font
            .glyphs
            .get(self.glyph_name.as_str())
            .ok_or_else(|| Error::NoGlyphForName(self.glyph_name.clone()))?;

        let mut ir_glyph = ir::GlyphBuilder::new(self.glyph_name.clone());
        ir_glyph.emit_to_binary = glyph.exported;
        ir_glyph.codepoints = glyph.codepoints.iter().copied().collect();

        let mut ir_anchors = AnchorBuilder::new(self.glyph_name.clone());
        let layers: Vec<&Layer> = glyph.layers.iter().collect();

        // Glyphs have layers that match up with masters, and masters have locations
        let mut axis_positions: HashMap<Tag, HashSet<NormalizedCoord>> = HashMap::new();
        for layer in layers.iter() {
            let maybe_location = layer.effective_location(&self.font);
            let design_location = if let Some(loc) = maybe_location {
                loc.clone()
            } else {
                if !self.options.produce_varc_table || !layer.is_smart_composite() {
                    log::warn!(
                        "Layer {} for glyph {} missing location info, skipping",
                        layer.debug_name(),
                        self.glyph_name
                    );
                    continue;
                }
                Location::new()
            };
            let location = design_location.to_normalized(axes)?;
            if self.options.skip_outlines && !location.is_default() {
                continue;
            }

            let (location, instance) = process_layer(glyph, &location, layer, &self.options)?;

            for (tag, coord) in location.iter() {
                axis_positions.entry(*tag).or_default().insert(*coord);
            }
            ir_glyph.try_add_source(&location, instance)?;

            // we only care about anchors from exportable glyphs
            // https://github.com/googlefonts/fontc/issues/1397
            if glyph.exported && !self.options.skip_outlines {
                for anchor in layer.anchors.iter() {
                    ir_anchors.add(
                        anchor.name.clone().into(),
                        location.clone(),
                        (anchor.x, anchor.y).into(),
                    )?;
                }
            }
        }
        // Let's do our own sanity check for default locations
        let locations = ir_glyph.sources.keys().cloned().collect::<Vec<_>>();
        if !locations.iter().any(|loc| loc.is_default()) {
            log::error!(
                "Glyph '{}' has no layer at default location: locations found: {:?}",
                self.glyph_name,
                locations
            );
            return Err(Error::BadGlyph(BadGlyph::new(
                &self.glyph_name,
                BadGlyphKind::NoDefaultLocation,
            )));
        }

        // It's helpful if glyphs are defined at default
        for axis in axes.iter() {
            let default = axis.default.to_normalized(&axis.converter);
            let positions = axis_positions.get(&axis.tag).ok_or_else(|| {
                BadGlyph::new(&self.glyph_name, BadGlyphKind::NoAxisPosition(axis.tag))
            })?;
            check_pos(&self.glyph_name, positions, axis, &default)?;
        }

        let ir_glyph = ir_glyph.build()?;
        let anchors = ir_anchors.build()?;

        //TODO: expand kerning to brackets

        context.anchors.set(anchors);
        context.glyphs.set(ir_glyph);
        Ok(())
    }
}

fn process_layer(
    glyph: &crate::Glyph,
    location: &NormalizedLocation,
    layer: &Layer,
    options: &CompilationOptions,
) -> Result<(NormalizedLocation, GlyphInstance), Error> {
    // See https://github.com/googlefonts/glyphsLib/blob/c4db6b98/Lib/glyphsLib/builder/glyph.py#L359-L389
    // let local_metrics = global_metrics.at(location);
    // let height = instance
    //     .vert_width
    //     .unwrap_or_else(|| local_metrics.os2_typo_ascender - local_metrics.os2_typo_descender)
    //     .into_inner();
    // let vertical_origin = instance
    //     .vert_origin
    //     .map(|origin| local_metrics.os2_typo_ascender - origin)
    //     .unwrap_or(local_metrics.os2_typo_ascender)
    //     .into_inner();

    // TODO populate width and height properly
    let (contours, components) = if options.skip_outlines {
        (vec![], vec![])
    } else {
        to_ir_contours_and_components(glyph.name.clone().into(), &layer.shapes)?
    };
    // If it's a smart composite layer, and we're doing VARC, drop components
    // (they'll go in the VARC table later)
    let components = if layer.is_smart_composite() && options.produce_varc_table {
        vec![]
    } else {
        components
    };
    let glyph_instance = GlyphInstance {
        // XXX https://github.com/googlefonts/fontmake-rs/issues/285 glyphs non-spacing marks are 0-width
        width: layer.width.into(),
        height: None,
        vertical_origin: None,
        // height: Some(height),
        // vertical_origin: Some(vertical_origin),
        contours,
        components,
    };
    Ok((location.clone(), glyph_instance))
}

pub(crate) fn to_ir_contours_and_components(
    glyph_name: GlyphName,
    shapes: &[Shape],
) -> Result<(Vec<BezPath>, Vec<ir::Component>), BadGlyph> {
    // For most glyphs in most fonts all the shapes are contours so it's a good guess
    let mut contours = Vec::with_capacity(shapes.len());
    let mut components = Vec::new();

    for shape in shapes.iter() {
        match shape {
            Shape::Component(component) => {
                components.push(to_ir_component(glyph_name.clone(), component))
            }
            Shape::Path(path) => contours.push(
                to_ir_path(glyph_name.clone(), path)
                    .map_err(|e| BadGlyph::new(glyph_name.clone(), e))?,
            ),
        }
    }

    Ok((contours, components))
}

fn to_ir_component(glyph_name: GlyphName, component: &Component) -> ir::Component {
    log::trace!(
        "{} reuses {} with transform {:?}",
        glyph_name,
        component.reference,
        component.transform
    );
    ir::Component {
        base: component.reference.as_str().into(),
        transform: component.transform.as_affine(),
    }
}

fn add_to_path<'a>(
    path_builder: &'a mut GlyphPathBuilder,
    nodes: impl Iterator<Item = &'a crate::Node>,
) -> Result<(), PathConversionError> {
    for node in nodes {
        match node.nodetype {
            NodeType::Move => path_builder.move_to((node.x, node.y)),
            NodeType::Line => path_builder.line_to((node.x, node.y)),
            NodeType::Curve => path_builder.curve_to((node.x, node.y)),
            NodeType::OffCurve => path_builder.offcurve((node.x, node.y)),
            NodeType::QCurve => path_builder.qcurve_to((node.x, node.y)),
        }?
    }
    Ok(())
}

fn to_ir_path(
    glyph_name: GlyphName,
    src_path: &crate::Path,
) -> Result<BezPath, PathConversionError> {
    // Based on https://github.com/googlefonts/glyphsLib/blob/24b4d340e4c82948ba121dcfe563c1450a8e69c9/Lib/glyphsLib/builder/paths.py#L20
    // See also https://github.com/fonttools/ufoLib2/blob/4d8a9600148b670b0840120658d9aab0b38a9465/src/ufoLib2/pointPens/glyphPointPen.py#L16
    if src_path.nodes.is_empty() {
        return Ok(BezPath::new());
    }

    let mut path_builder = GlyphPathBuilder::new(src_path.nodes.len());

    // First is a delicate butterfly
    if !src_path.closed {
        if let Some(first) = src_path.nodes.first() {
            if first.nodetype == NodeType::OffCurve {
                return Err(PathConversionError::Parse(
                    "Open path starts with off-curve points".into(),
                ));
            }
            path_builder.move_to((first.x, first.y))?;
            add_to_path(&mut path_builder, src_path.nodes[1..].iter())?;
        }
    } else if src_path
        .nodes
        .iter()
        .any(|node| node.nodetype != NodeType::OffCurve)
    {
        // In Glyphs.app, the starting node of a closed contour is always
        // stored at the end of the nodes list.
        // Rotate right by 1 by way of chaining iterators
        let last_idx = src_path.nodes.len() - 1;
        add_to_path(
            &mut path_builder,
            std::iter::once(&src_path.nodes[last_idx]).chain(&src_path.nodes[..last_idx]),
        )?;
    } else {
        // except if the contour contains only off-curve points (implied quadratic)
        // in which case we're already in the correct order (this is very rare
        // in glyphs sources and might be the result of bugs, but it exists)
        add_to_path(&mut path_builder, src_path.nodes.iter())?;
    };

    // if path_builder.erase_open_corners()? {
    //     log::debug!("erased open contours for {glyph_name}");
    // }

    let path = path_builder.build()?;

    log::trace!(
        "Built a {} entry path for {glyph_name}",
        path.elements().len(),
    );
    Ok(path)
}
