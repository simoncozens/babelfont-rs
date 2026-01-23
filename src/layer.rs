use crate::{
    anchor::Anchor,
    common::{Color, FormatSpecific},
    guide::Guide,
    shape::Shape,
    BabelfontError, Component, Font, Node, Path,
};
use fontdrasil::coords::{DesignCoord, DesignLocation};
use indexmap::IndexMap;
use kurbo::Shape as KurboShape;
use serde::{Deserialize, Serialize};
use typeshare::typeshare;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", content = "master")]
#[typeshare]
/// The type of a layer in relation to masters
pub enum LayerType {
    /// A default layer for a master
    DefaultForMaster(String),
    /// A layer associated with a master but not the default
    AssociatedWithMaster(String),
    /// A free-floating layer not associated with any master
    #[default]
    FreeFloating,
}
impl LayerType {
    fn is_default(&self) -> bool {
        matches!(self, LayerType::FreeFloating)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[typeshare]
/// A layer of a glyph in a font
pub struct Layer {
    /// The advance width of the layer
    pub width: f32,
    /// The name of the layer
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The ID of the layer
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The relationship between this layer and a master, if any
    #[serde(default, skip_serializing_if = "LayerType::is_default")]
    pub master: LayerType,
    /// Guidelines in the layer
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guides: Vec<Guide>,
    /// Shapes (paths and components) in the layer
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shapes: Vec<Shape>,
    /// Anchors in the layer
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub anchors: Vec<Anchor>,
    /// The color of the layer
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    /// The index of the layer in a color font
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer_index: Option<i32>,
    /// Whether this layer is a background layer
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_background: bool,
    /// The ID of the background layer for this layer, if any
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background_layer_id: Option<String>,
    /// The location of the layer in design space, if it is not at the default location for a master
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serde_helpers::option_design_location_to_map",
        deserialize_with = "crate::serde_helpers::option_design_location_from_map"
    )]
    #[typeshare(python(type = "Optional[Dict[str, float]]"))]
    #[typeshare(typescript(type = "import('@simoncozens/fonttypes').DesignspaceLocation"))]
    pub location: Option<DesignLocation>,
    /// The location of the layer in smart component (glyph-specific axes) space
    // Note we don't use DesignLocation here because smart component locations are not
    // named by axis tags but by axis names.
    #[serde(
        default,
        skip_serializing_if = "IndexMap::is_empty",
        serialize_with = "crate::serde_helpers::string_design_location_to_map",
        deserialize_with = "crate::serde_helpers::string_design_location_from_map"
    )]
    #[typeshare(python(type = "Optional[Dict[str, float]]"))]
    #[typeshare(typescript(type = "import('@simoncozens/fonttypes').DesignspaceLocation"))]
    pub smart_component_location: IndexMap<String, DesignCoord>,

    /// Format-specific data for the layer
    #[serde(default, skip_serializing_if = "FormatSpecific::is_empty")]
    #[typeshare(python(type = "Dict[str, Any]"))]
    #[typeshare(typescript(type = "Record<string, any>"))]
    pub format_specific: FormatSpecific,
}

impl Layer {
    /// Create a new layer with the given advance width
    pub fn new(width: f32) -> Layer {
        Layer {
            width,
            ..Default::default()
        }
    }

    /// Iterate over the components in the layer
    pub fn components(&self) -> impl DoubleEndedIterator<Item = &Component> {
        self.shapes.iter().filter_map(|x| {
            if let Shape::Component(c) = x {
                Some(c)
            } else {
                None
            }
        })
    }

    /// Iterate over the paths in the layer
    pub fn paths(&self) -> impl DoubleEndedIterator<Item = &Path> {
        self.shapes.iter().filter_map(|x| {
            if let Shape::Path(p) = x {
                Some(p)
            } else {
                None
            }
        })
    }

    /// Clear all components from the layer
    pub fn clear_components(&mut self) {
        self.shapes.retain(|sh| matches!(sh, Shape::Path(_)));
    }

    /// Add a component to the layer
    pub fn push_component(&mut self, c: Component) {
        self.shapes.push(Shape::Component(c))
    }
    /// Add a path to the layer
    pub fn push_path(&mut self, p: Path) {
        self.shapes.push(Shape::Path(p))
    }

    /// Check if the layer has any components
    pub fn has_components(&self) -> bool {
        self.shapes
            .iter()
            .any(|sh| matches!(sh, Shape::Component(_)))
    }

    /// Check if the layer has any paths
    pub fn has_paths(&self) -> bool {
        self.shapes.iter().any(|sh| matches!(sh, Shape::Path(_)))
    }

    /// Decompose all components in the layer, replacing them with their decomposed paths
    pub fn decompose(&mut self, font: &Font) {
        let decomposed_shapes = self
            .decomposed_components(font)
            .into_iter()
            .map(Shape::Path);
        self.shapes.retain(|sh| matches!(sh, Shape::Path(_)));
        self.shapes.extend(decomposed_shapes);
    }

    /// Return a new layer with all components decomposed into paths
    pub fn decomposed(&self, font: &Font) -> Layer {
        let decomposed_shapes = self
            .decomposed_components(font)
            .into_iter()
            .map(Shape::Path);
        Layer {
            width: self.width,
            name: self.name.clone(),
            id: self.id.clone(),
            master: self.master.clone(),
            guides: self.guides.clone(),
            anchors: self.anchors.clone(),
            color: self.color,
            layer_index: self.layer_index,
            is_background: self.is_background,
            background_layer_id: self.background_layer_id.clone(),
            location: self.location.clone(),
            smart_component_location: self.smart_component_location.clone(),
            shapes: self
                .shapes
                .iter()
                .filter(|sh| matches!(sh, Shape::Path(_)))
                .cloned()
                .chain(decomposed_shapes)
                .collect(),
            format_specific: self.format_specific.clone(),
        }
    }

    /// Return a vector of decomposed paths from all components in the layer
    pub fn decomposed_components(&self, font: &Font) -> Vec<Path> {
        let mut contours = Vec::new();

        let mut stack: Vec<(&Component, kurbo::Affine)> = Vec::new();
        for component in self.components() {
            stack.push((component, component.transform.as_affine()));
            while let Some((component, transform)) = stack.pop() {
                let referenced_glyph = match font.glyphs.get(&component.reference) {
                    Some(g) => g,
                    None => continue,
                };
                let new_outline = match self
                    .id
                    .as_ref()
                    .and_then(|id| referenced_glyph.get_layer(id))
                {
                    Some(g) => g,
                    None => continue,
                };

                for contour in new_outline.paths() {
                    let mut decomposed_contour = Path::default();
                    for node in &contour.nodes {
                        let new_point = transform * kurbo::Point::new(node.x, node.y);
                        decomposed_contour.nodes.push(Node {
                            x: new_point.x,
                            y: new_point.y,
                            nodetype: node.nodetype,
                            smooth: node.smooth,
                            format_specific: node.format_specific.clone(),
                        })
                    }
                    decomposed_contour.closed = contour.closed;
                    contours.push(decomposed_contour);
                }

                // Depth-first decomposition means we need to extend the stack reversed, so
                // the first component is taken out next.
                for new_component in new_outline.components().rev() {
                    let new_transform: kurbo::Affine = new_component.transform.as_affine();
                    stack.push((new_component, transform * new_transform));
                }
            }
        }

        contours
    }

    /// Calculate the bounding box of the layer
    ///
    /// If the layer has components, an error is returned and the layer must be decomposed first
    pub fn bounds(&self) -> Result<crate::Rect, BabelfontError> {
        if self.has_components() {
            return Err(BabelfontError::NeedsDecomposition);
        }
        let paths: Result<Vec<kurbo::BezPath>, BabelfontError> =
            self.paths().map(|p| p.to_kurbo()).collect();
        let bbox: kurbo::Rect = paths?
            .iter()
            .map(|p| p.bounding_box())
            .reduce(|accum, item| accum.union(item))
            .unwrap_or_default();
        Ok(bbox)
    }

    /// Calculate the left side bearing of the layer
    ///
    /// If the layer has components, an error is returned and the layer must be decomposed first
    pub fn lsb(&self) -> Result<f32, BabelfontError> {
        let bounds: kurbo::Rect = self.bounds()?;
        Ok(bounds.min_x() as f32)
    }

    /// Calculate the right side bearing of the layer
    ///
    /// If the layer has components, an error is returned and the layer must be decomposed first
    pub fn rsb(&self) -> Result<f32, BabelfontError> {
        let bounds = self.bounds()?;
        Ok(self.width - bounds.max_x() as f32)
    }

    /// Is the layer a smart composite? (i.e. contains smart/variable components)
    pub fn is_smart_composite(&self) -> bool {
        self.shapes.iter().any(|shape| match shape {
            Shape::Component(c) => !c.location.is_empty(),
            _ => false,
        })
    }

    pub(crate) fn debug_name(&self) -> String {
        if let Some(name) = &self.name {
            name.clone()
        } else if let Some(id) = &self.id {
            id.clone()
        } else {
            "Unnamed Layer".to_string()
        }
    }
}

// kurbo pen protocol support?

#[cfg(feature = "glyphs")]
pub(crate) mod glyphs {
    use crate::convertors::glyphs3::{
        copy_user_data, KEY_ATTR, KEY_COLOR_LABEL, KEY_METRIC_BOTTOM, KEY_METRIC_LEFT,
        KEY_METRIC_RIGHT, KEY_METRIC_TOP, KEY_METRIC_VERT_WIDTH, KEY_METRIC_WIDTH, KEY_VERT_ORIGIN,
        KEY_VERT_WIDTH,
    };
    use std::collections::BTreeMap;

    use fontdrasil::{coords::DesignCoord, types::Tag};
    use glyphslib::Plist;
    use smol_str::SmolStr;

    use crate::convertors::glyphs3::{
        UserData, KEY_ANNOTATIONS, KEY_LAYER_HINTS, KEY_LAYER_IMAGE, KEY_USER_DATA,
    };

    use super::*;

    pub(crate) fn layer_from_glyphs(
        val: &glyphslib::glyphs3::Layer,
        axes_order: &[Tag],
        glyph_specific_axes: &[(String, DesignCoord, DesignCoord)], // name, bottom, top
    ) -> Result<Layer, BabelfontError> {
        let format_specific = {
            let mut fs = FormatSpecific::default();
            fs.insert_if_ne_json("visible", &val.visible, &true);
            fs.insert_nonempty_json(KEY_LAYER_HINTS, &val.hints);
            fs.insert_nonempty_json(KEY_ANNOTATIONS, &val.annotations);
            fs.insert_some_json(KEY_LAYER_IMAGE, &val.background_image);
            fs.insert_some_json(KEY_VERT_ORIGIN, &val.vert_origin);
            fs.insert_some_json(KEY_VERT_WIDTH, &val.vert_width);
            fs.insert_some_json(KEY_METRIC_WIDTH, &val.metric_width);
            fs.insert_some_json(KEY_METRIC_VERT_WIDTH, &val.metric_vert_width);
            fs.insert_some_json(KEY_METRIC_TOP, &val.metric_top);
            fs.insert_some_json(KEY_METRIC_RIGHT, &val.metric_right);
            fs.insert_some_json(KEY_METRIC_LEFT, &val.metric_left);
            // Note this is the color of the label, not the index of the color
            // palette in a color font
            fs.insert_some_json(KEY_COLOR_LABEL, &val.color);
            fs.insert_some_json(KEY_METRIC_BOTTOM, &val.metric_bottom);
            copy_user_data(&mut fs, &val.user_data);
            fs.insert_json(KEY_ATTR, &val.attr);
            fs
        };
        let location = val
            .attr
            .get("coordinates")
            .and_then(|x| x.as_array())
            .map(|coords| {
                axes_order
                    .iter()
                    .zip(coords.iter())
                    .filter_map(|(axis_tag, v)| {
                        v.as_f64().map(|f| (*axis_tag, DesignCoord::new(f)))
                    })
                    .collect::<DesignLocation>()
            });
        let mut smart_location = IndexMap::new();
        for (axis, index) in val.part_selection.iter() {
            if let Some((_, bottom, top)) =
                glyph_specific_axes.iter().find(|(name, _, _)| name == axis)
            {
                let coord = match index {
                    1 => *bottom,
                    2 => *top,
                    _ => continue,
                };
                smart_location.insert(axis.clone(), coord);
            } else {
                return Err(BabelfontError::UnknownSmartComponentAxis {
                    axis: axis.clone(),
                    layer: val.layer_id.clone(),
                });
            }
        }
        Ok(Layer {
            id: Some(val.layer_id.clone()),
            master: match &val.associated_master_id {
                Some(m) => LayerType::AssociatedWithMaster(m.clone()),
                None => LayerType::DefaultForMaster(val.layer_id.clone()),
            },
            name: val.name.clone(),
            color: None,
            shapes: val.shapes.iter().map(Into::into).collect(),
            width: val.width,
            guides: val.guides.iter().map(Into::into).collect(),
            anchors: val.anchors.iter().map(Into::into).collect(),
            layer_index: None,
            is_background: false,
            background_layer_id: None,
            location,
            smart_component_location: smart_location,
            format_specific,
        })
    }

    pub(crate) fn layer_to_glyphs(
        val: &Layer,
        axes_order: &[Tag],
        glyph_specific_axes: &[(String, DesignCoord, DesignCoord)],
    ) -> glyphslib::glyphs3::Layer {
        let mut attr: BTreeMap<SmolStr, _> = BTreeMap::new();
        if let Some(attr_map) = val
            .format_specific
            .get_parse_opt::<BTreeMap<SmolStr, Plist>>(KEY_ATTR)
        {
            attr.extend(attr_map);
        }
        if let Some(coords) = &val.location {
            attr.insert(
                "coordinates".into(),
                axes_order
                    .iter()
                    .map(|axis_tag| coords.get(*axis_tag).map(|x| x.to_f64()).unwrap_or(0.0))
                    .collect::<Vec<_>>()
                    .into(),
            );
        }
        let mut part_selection: BTreeMap<String, u8> = BTreeMap::new();
        for (axis, bottom, top) in glyph_specific_axes {
            if let Some(coord) = val.smart_component_location.get(axis) {
                if *coord == *bottom {
                    part_selection.insert(axis.clone(), 1);
                } else if *coord == *top {
                    part_selection.insert(axis.clone(), 2);
                }
            }
        }
        glyphslib::glyphs3::Layer {
            layer_id: match val.master {
                LayerType::DefaultForMaster(ref m) => m.clone(),
                _ => val.id.clone().unwrap_or_default(),
            },
            name: val.name.clone(),
            width: val.width,
            shapes: val.shapes.iter().map(Into::into).collect(),
            guides: val.guides.iter().map(Into::into).collect(),
            anchors: val.anchors.iter().map(Into::into).collect(),
            annotations: val
                .format_specific
                .get_parse_or::<Vec<BTreeMap<SmolStr, Plist>>>(KEY_ANNOTATIONS, Vec::new()),
            associated_master_id: match val.master {
                LayerType::AssociatedWithMaster(ref m) => Some(m.clone()),
                _ => None,
            },
            attr,
            background: None,
            background_image: val
                .format_specific
                .get_parse_opt::<glyphslib::glyphs3::BackgroundImage>(KEY_LAYER_IMAGE),
            color: val
                .format_specific
                .get_parse_opt::<u8>(KEY_COLOR_LABEL)
                .map(glyphslib::common::Color::ColorInt),
            hints: val
                .format_specific
                .get_parse_or::<Vec<BTreeMap<SmolStr, Plist>>>(KEY_LAYER_HINTS, Vec::new()),
            metric_bottom: val.format_specific.get_optionstring(KEY_METRIC_BOTTOM),
            metric_left: val.format_specific.get_optionstring(KEY_METRIC_LEFT),
            metric_right: val.format_specific.get_optionstring(KEY_METRIC_RIGHT),
            metric_top: val.format_specific.get_optionstring(KEY_METRIC_TOP),
            metric_vert_width: val.format_specific.get_optionstring(KEY_METRIC_VERT_WIDTH),
            metric_width: val.format_specific.get_optionstring(KEY_METRIC_WIDTH),
            part_selection,
            user_data: val
                .format_specific
                .get(KEY_USER_DATA)
                .and_then(|x| serde_json::from_value::<UserData>(x.clone()).ok())
                .unwrap_or_default(),
            vert_origin: val.format_specific.get_parse_opt::<f32>(KEY_VERT_ORIGIN),
            vert_width: val.format_specific.get_parse_opt::<f32>(KEY_VERT_WIDTH),
            visible: val.format_specific.get_bool_or("visible", true),
        }
    }
}

#[cfg(feature = "fontra")]
mod fontra {
    use super::*;
    use crate::convertors::fontra;

    impl From<&Layer> for fontra::Layer {
        fn from(val: &Layer) -> Self {
            let mut path = fontra::PackedPath::default();
            for p in val.paths() {
                path.push_path(p);
            }

            fontra::Layer {
                glyph: fontra::StaticGlyph {
                    path,
                    components: val.components().map(|c| c.into()).collect(),
                    x_advance: val.width,
                    y_advance: 0.0,
                    anchors: val.anchors.iter().map(|a| a.into()).collect(),
                    guides: vec![],
                },
            }
        }
    }
}
