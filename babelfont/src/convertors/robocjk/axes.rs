use fontdrasil::coords::{DesignCoord, UserCoord};

use crate::{common::tag_from_string, Axis, BabelfontError};

/// Convert a fontra::Axis to a babelfont::Axis
pub(crate) fn axis_from_fontra(
    fontra_axis: &crate::convertors::fontra::Axis,
) -> Result<Axis, BabelfontError> {
    let tag = tag_from_string(&fontra_axis.tag)?;
    let mut axis = Axis::new(fontra_axis.name.clone(), tag);

    axis.min = Some(UserCoord::new(fontra_axis.min_value));
    axis.max = Some(UserCoord::new(fontra_axis.max_value));
    axis.default = Some(UserCoord::new(fontra_axis.default_value));
    axis.hidden = fontra_axis.hidden;

    // Convert the mapping from (UserCoord, NormalizedCoord) to (UserCoord, DesignCoord)
    if !fontra_axis.mapping.is_empty() {
        let map: Vec<(UserCoord, DesignCoord)> = fontra_axis
            .mapping
            .iter()
            .map(|(user, normalized)| {
                // Assume design space = normalized space
                (*user, DesignCoord::new(normalized.to_f64()))
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
