use crate::common::decomposition::{DecomposedAffine, TransformOrder};

/// Parse a robocjk.deepComponents transform into a DecomposedAffine
pub(crate) fn parse_deep_component_transform(
    transform_obj: &serde_json::Value,
) -> DecomposedAffine {
    let x = transform_obj
        .get("x")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let y = transform_obj
        .get("y")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let scalex = transform_obj
        .get("scalex")
        .and_then(|v| v.as_f64())
        .unwrap_or(1.0);
    let scaley = transform_obj
        .get("scaley")
        .and_then(|v| v.as_f64())
        .unwrap_or(1.0);
    let rotation = transform_obj
        .get("rotation")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    // tcenterx and tcentery are transformation center offsets, but we don't
    // have direct support for them in DecomposedAffine, so we skip them for now
    let _tcenterx = transform_obj
        .get("tcenterx")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let _tcentery = transform_obj
        .get("tcentery")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);

    DecomposedAffine {
        translation: (x, y),
        skew: (0.0, 0.0),
        rotation: rotation.to_radians(),
        scale: (scalex, scaley),
        order: TransformOrder::Glyphs,
    }
}
