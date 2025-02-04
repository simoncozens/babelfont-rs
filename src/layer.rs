use crate::{
    anchor::Anchor, common::Color, guide::Guide, shape::Shape, BabelfontError, Component, Font,
    Node, Path,
};
use fontdrasil::coords::DesignLocation;
use kurbo::Shape as KurboShape;

#[derive(Debug, Clone)]
pub struct Layer {
    pub width: f32,
    pub name: Option<String>,
    pub id: Option<String>,
    pub guides: Vec<Guide>,
    pub shapes: Vec<Shape>,
    pub anchors: Vec<Anchor>,
    pub color: Option<Color>,
    pub layer_index: Option<i32>,
    pub is_background: bool,
    pub background_layer_id: Option<String>,
    pub location: Option<DesignLocation>,
}

impl Layer {
    pub fn new(width: f32) -> Layer {
        Layer {
            width,
            name: None,
            id: None,
            guides: vec![],
            shapes: vec![],
            anchors: vec![],
            color: None,
            layer_index: None,
            is_background: false,
            background_layer_id: None,
            location: None,
        }
    }

    pub fn components(&self) -> impl DoubleEndedIterator<Item = &Component> {
        self.shapes.iter().filter_map(|x| {
            if let Shape::ComponentShape(c) = x {
                Some(c)
            } else {
                None
            }
        })
    }

    pub fn paths(&self) -> impl DoubleEndedIterator<Item = &Path> {
        self.shapes.iter().filter_map(|x| {
            if let Shape::PathShape(p) = x {
                Some(p)
            } else {
                None
            }
        })
    }

    pub fn clear_components(&mut self) {
        self.shapes.retain(|sh| matches!(sh, Shape::PathShape(_)));
    }

    pub fn push_component(&mut self, c: Component) {
        self.shapes.push(Shape::ComponentShape(c))
    }

    pub fn push_path(&mut self, p: Path) {
        self.shapes.push(Shape::PathShape(p))
    }

    pub fn has_components(&self) -> bool {
        self.shapes
            .iter()
            .any(|sh| matches!(sh, Shape::ComponentShape(_)))
    }

    pub fn has_paths(&self) -> bool {
        self.shapes
            .iter()
            .any(|sh| matches!(sh, Shape::PathShape(_)))
    }

    pub fn decompose(&mut self, font: &Font) {
        let decomposed_shapes = self
            .decomposed_components(font)
            .into_iter()
            .map(Shape::PathShape);
        self.shapes.retain(|sh| matches!(sh, Shape::PathShape(_)));
        self.shapes.extend(decomposed_shapes);
    }

    pub fn decomposed(&self, font: &Font) -> Layer {
        let decomposed_shapes = self
            .decomposed_components(font)
            .into_iter()
            .map(Shape::PathShape);
        Layer {
            width: self.width,
            name: self.name.clone(),
            id: self.id.clone(),
            guides: self.guides.clone(),
            anchors: self.anchors.clone(),
            color: self.color,
            layer_index: self.layer_index,
            is_background: self.is_background,
            background_layer_id: self.background_layer_id.clone(),
            location: self.location.clone(),
            shapes: self
                .shapes
                .iter()
                .filter(|sh| matches!(sh, Shape::PathShape(_)))
                .cloned()
                .chain(decomposed_shapes)
                .collect(),
        }
    }

    pub fn decomposed_components(&self, font: &Font) -> Vec<Path> {
        let mut contours = Vec::new();

        let mut stack: Vec<(&Component, kurbo::Affine)> = Vec::new();
        for component in self.components() {
            stack.push((component, component.transform));
            while let Some((component, transform)) = stack.pop() {
                let referenced_glyph = match font.glyphs.get(&component.reference) {
                    Some(g) => g,
                    None => continue,
                };
                #[warn(clippy::unwrap_used)]
                let new_outline = match referenced_glyph.get_layer(self.id.as_ref().unwrap()) {
                    Some(g) => g,
                    None => continue,
                };

                for contour in new_outline.paths() {
                    let mut decomposed_contour = Path::default();
                    for node in &contour.nodes {
                        let new_point = transform * kurbo::Point::new(node.x as f64, node.y as f64);
                        decomposed_contour.nodes.push(Node {
                            x: new_point.x as f32,
                            y: new_point.y as f32,
                            nodetype: node.nodetype,
                        })
                    }
                    decomposed_contour.closed = contour.closed;
                    contours.push(decomposed_contour);
                }

                // Depth-first decomposition means we need to extend the stack reversed, so
                // the first component is taken out next.
                for new_component in new_outline.components().rev() {
                    let new_transform: kurbo::Affine = new_component.transform;
                    stack.push((new_component, transform * new_transform));
                }
            }
        }

        contours
    }

    pub fn bounds(&self) -> Result<kurbo::Rect, BabelfontError> {
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

    pub fn lsb(&self) -> Result<f32, BabelfontError> {
        let bounds = self.bounds()?;
        Ok(bounds.min_x() as f32)
    }
    pub fn rsb(&self) -> Result<f32, BabelfontError> {
        let bounds = self.bounds()?;
        Ok(self.width - bounds.max_x() as f32)
    }
}

#[cfg(feature = "glyphs")]
mod glyphs {
    use std::collections::BTreeMap;

    use super::*;

    impl From<&glyphslib::glyphs3::Layer> for Layer {
        fn from(val: &glyphslib::glyphs3::Layer) -> Self {
            Layer {
                id: Some(val.layer_id.clone()),
                name: val.name.clone(),
                color: None,
                shapes: val.shapes.iter().map(Into::into).collect(),
                width: val.width,
                guides: val.guides.iter().map(Into::into).collect(),
                anchors: val.anchors.iter().map(Into::into).collect(),
                layer_index: None,
                is_background: false,
                background_layer_id: None,
                location: None,
            }
        }
    }

    impl From<&Layer> for glyphslib::glyphs3::Layer {
        fn from(val: &Layer) -> Self {
            glyphslib::glyphs3::Layer {
                layer_id: val.id.clone().unwrap_or_default(),
                name: val.name.clone(),
                width: val.width,
                shapes: val.shapes.iter().map(Into::into).collect(),
                guides: val.guides.iter().map(Into::into).collect(),
                anchors: val.anchors.iter().map(Into::into).collect(),
                annotations: vec![],
                associated_master_id: None,
                attr: BTreeMap::new(),
                background: None,
                background_image: None,
                color: None,
                hints: vec![],
                metric_bottom: None,
                metric_left: None,
                metric_right: None,
                metric_top: None,
                metric_vert_width: None,
                metric_width: None,
                part_selection: BTreeMap::new(),
                user_data: BTreeMap::new(),
                vert_origin: None,
                vert_width: None,
                visible: false,
            }
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
