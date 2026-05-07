use kurbo::{BezPath, PathEl, QuadBez};

use crate::{filters::FontFilter, Node, Path};

use super::curve_filter_common::{
    apply_interpolatable_path_filter, mark_closed_and_normalize, path_el_kind,
};

/// A filter that converts quadratic Bézier curves to cubic Bézier curves in all glyphs of a font, attempting to keep corresponding paths across layers consistent for better interpolation results. This filter requires the `kurbo` feature to be enabled.
#[derive(Debug, Clone, Default)]
pub struct QuadraticToCubic;

fn convert_bezpaths_in_parallel(paths: Vec<&BezPath>) -> Result<Vec<Path>, crate::BabelfontError> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }

    let mut new_paths = vec![Path::default(); paths.len()];
    let mut all_elements = Vec::with_capacity(paths.len());

    for path in &paths {
        all_elements.push(path.elements());
    }

    if all_elements.iter().all(|elements| elements.is_empty()) {
        return Ok(new_paths);
    }
    if all_elements.iter().any(|elements| elements.is_empty()) {
        return Err(crate::BabelfontError::FilterError(
            "Parallel conversion requires all paths to be either empty or non-empty".to_string(),
        ));
    }

    let mut last_points = Vec::with_capacity(paths.len());
    for elements in &all_elements {
        let last_point = if let Some(PathEl::MoveTo(p)) = elements.first() {
            Some(*p)
        } else {
            // Closed contours in kurbo may not start with a move command.
            elements.last().and_then(|el| el.end_point())
        };
        let Some(last_point) = last_point else {
            return Err(crate::BabelfontError::FilterError(
                "Cannot determine starting point for path during parallel conversion".to_string(),
            ));
        };
        last_points.push(last_point);
    }

    let first_len = all_elements[0].len();
    if all_elements
        .iter()
        .any(|elements| elements.len() != first_len)
    {
        return Err(crate::BabelfontError::FilterError(
            "Parallel conversion requires all paths to have the same number of elements"
                .to_string(),
        ));
    }

    for el_ix in 0..first_len {
        let expected_kind = path_el_kind(&all_elements[0][el_ix]);
        if all_elements
            .iter()
            .any(|elements| path_el_kind(&elements[el_ix]) != expected_kind)
        {
            return Err(crate::BabelfontError::FilterError(format!(
                "Parallel conversion requires matching element kinds at index {}",
                el_ix
            )));
        }

        match &all_elements[0][el_ix] {
            PathEl::MoveTo(_) => {
                for (path_ix, elements) in all_elements.iter().enumerate() {
                    let PathEl::MoveTo(p) = &elements[el_ix] else {
                        unreachable!("element kind checked above")
                    };
                    new_paths[path_ix].nodes.push(Node::new_move(p.x, p.y));
                    last_points[path_ix] = *p;
                }
            }
            PathEl::LineTo(_) => {
                for (path_ix, elements) in all_elements.iter().enumerate() {
                    let PathEl::LineTo(p) = &elements[el_ix] else {
                        unreachable!("element kind checked above")
                    };
                    new_paths[path_ix].nodes.push(Node::new_line(p.x, p.y));
                    last_points[path_ix] = *p;
                }
            }
            PathEl::QuadTo(_, _) => {
                for (path_ix, elements) in all_elements.iter().enumerate() {
                    let PathEl::QuadTo(p1, p2) = &elements[el_ix] else {
                        unreachable!("element kind checked above")
                    };
                    let quadbez = QuadBez::new(last_points[path_ix], *p1, *p2);
                    let cubicbez = quadbez.raise();
                    new_paths[path_ix]
                        .nodes
                        .push(Node::new_offcurve(cubicbez.p1.x, cubicbez.p1.y));
                    new_paths[path_ix]
                        .nodes
                        .push(Node::new_offcurve(cubicbez.p2.x, cubicbez.p2.y));
                    new_paths[path_ix]
                        .nodes
                        .push(Node::new_curve(cubicbez.p3.x, cubicbez.p3.y));
                    last_points[path_ix] = cubicbez.p3;
                }
            }
            PathEl::CurveTo(_, _, _) => {
                for (path_ix, elements) in all_elements.iter().enumerate() {
                    let PathEl::CurveTo(p1, p2, p3) = &elements[el_ix] else {
                        unreachable!("element kind checked above")
                    };
                    new_paths[path_ix]
                        .nodes
                        .push(Node::new_offcurve(p1.x, p1.y));
                    new_paths[path_ix]
                        .nodes
                        .push(Node::new_offcurve(p2.x, p2.y));
                    new_paths[path_ix].nodes.push(Node::new_curve(p3.x, p3.y));
                    last_points[path_ix] = *p3;
                }
            }
            PathEl::ClosePath => {
                mark_closed_and_normalize(&mut new_paths);
            }
        }
    }

    Ok(new_paths)
}

fn convert_bezpath_independently(path: &BezPath) -> Result<Path, crate::BabelfontError> {
    let converted = convert_bezpaths_in_parallel(vec![path])?;
    Ok(converted.into_iter().next().unwrap_or_default())
}

impl QuadraticToCubic {
    /// Create a new QuadraticToCubic filter
    pub fn new() -> Self {
        QuadraticToCubic
    }
}

impl FontFilter for QuadraticToCubic {
    fn apply(&self, font: &mut crate::Font) -> Result<(), crate::BabelfontError> {
        apply_interpolatable_path_filter(
            font,
            "QuadraticToCubic",
            convert_bezpath_independently,
            convert_bezpaths_in_parallel,
        )
    }

    fn from_str(_s: &str) -> Result<Self, crate::BabelfontError>
    where
        Self: Sized,
    {
        Ok(QuadraticToCubic::new())
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("quadratic2cubic")
            .long("quadratic-to-cubic")
            .help("Convert quadratic Bézier curves to cubic Bézier curves")
            .action(clap::ArgAction::SetTrue)
    }
}

#[allow(clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_final_segment() {
        let path: Path = serde_json::from_str(r#"
            {
              "nodes": "394 173 o 467 246 o 467 337 cs 467 427 o 394 500 o 304 500 cs 213 500 o 140 427 o 140 337 cs 140 246 o 213 173 o 304 173 cs",
              "closed": true
            }
"#).unwrap();
        let kurbo = path.to_kurbo().unwrap();
        let converted = convert_bezpath_independently(&kurbo).unwrap();
        assert!(converted.closed);
        // There should be no move node
        assert!(!converted
            .nodes
            .iter()
            .any(|n| n.nodetype == crate::NodeType::Move));
    }
}
