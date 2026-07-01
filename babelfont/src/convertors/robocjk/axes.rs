use fontdrasil::coords::{DesignCoord, UserCoord};

use crate::{common::tag_from_string, Axis, BabelfontError};

/// Extract a babelfont::Axis from a fontra::AnyAxis (handles both continuous and discrete).
pub(crate) fn axis_from_fontra(
    any_axis: &crate::convertors::fontra::AnyAxis,
) -> Result<Axis, BabelfontError> {
    match any_axis {
        crate::convertors::fontra::AnyAxis::Continuous(fa) => axis_from_continuous(fa),
        crate::convertors::fontra::AnyAxis::Discrete(da) => axis_from_discrete(da),
    }
}

fn axis_from_continuous(fontra_axis: &crate::convertors::fontra::FontAxis) -> Result<Axis, BabelfontError> {
    let tag = tag_from_string(&fontra_axis.tag)?;
    let mut axis = Axis::new(fontra_axis.name.clone(), tag);

    axis.min = Some(UserCoord::new(fontra_axis.min_value));
    axis.max = Some(UserCoord::new(fontra_axis.max_value));
    axis.default = Some(UserCoord::new(fontra_axis.default_value));
    axis.hidden = fontra_axis.hidden;

    // Convert the mapping from [[user, normalized], ...] to [(UserCoord, DesignCoord)]
    if !fontra_axis.mapping.is_empty() {
        let map: Vec<(UserCoord, DesignCoord)> = fontra_axis
            .mapping
            .iter()
            .map(|pair| {
                let user = UserCoord::new(pair[0]);
                let normalized = DesignCoord::new(pair[1]);
                (user, normalized)
            })
            .collect();
        axis.map = Some(map);
    }

    Ok(axis)
}

fn axis_from_discrete(discrete_axis: &crate::convertors::fontra::DiscreteFontAxis) -> Result<Axis, BabelfontError> {
    let tag = tag_from_string(&discrete_axis.tag)?;
    let mut axis = Axis::new(discrete_axis.name.clone(), tag);

    if !discrete_axis.values.is_empty() {
        axis.min = Some(UserCoord::new(
            *discrete_axis.values.iter().min_by(|a, b| a.partial_cmp(b).unwrap()).unwrap_or(&0.0),
        ));
        axis.max = Some(UserCoord::new(
            *discrete_axis.values.iter().max_by(|a, b| a.partial_cmp(b).unwrap()).unwrap_or(&0.0),
        ));
        axis.values = discrete_axis.values.iter().map(|&v| UserCoord::new(v)).collect();
    }
    axis.default = Some(UserCoord::new(discrete_axis.default_value));
    axis.hidden = discrete_axis.hidden;

    // Convert the mapping
    if !discrete_axis.mapping.is_empty() {
        let map: Vec<(UserCoord, DesignCoord)> = discrete_axis
            .mapping
            .iter()
            .map(|pair| {
                let user = UserCoord::new(pair[0]);
                let normalized = DesignCoord::new(pair[1]);
                (user, normalized)
            })
            .collect();
        axis.map = Some(map);
    }

    Ok(axis)
}

/// Extract RoboCJK smart component axes from a norad glyph lib
pub(crate) fn component_axes_from_lib(lib: &norad::Plist) -> Vec<Axis> {
    let mut axes: Vec<Axis> = Vec::new();
    let lib_json = serde_json::to_value(lib).unwrap_or_default();
    if let Some(arr) = lib_json.get("robocjk.axes").and_then(|v| v.as_array()) {
        for item in arr {
            if let Some(name) = item.get("name").and_then(|v| v.as_str()) {
                let min = item.get("minValue").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let max = item.get("maxValue").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let default = item
                    .get("defaultValue")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(min);
                let mut axis = Axis::new(name.to_string(), fontdrasil::types::Tag::new(b"VARC"));
                axis.min = Some(UserCoord::new(min));
                axis.max = Some(UserCoord::new(max));
                axis.default = Some(UserCoord::new(default));
                axes.push(axis);
            }
        }
    }
    axes
}
