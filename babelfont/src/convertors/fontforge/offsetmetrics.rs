use crate::{BabelfontError, Font, MetricType};

/// The font-wide y extremes over every master-default layer, with
/// component references resolved (true bezier bounds; beziers are affine
/// invariant, so transforming control points then taking curve bounds is
/// exact).
pub(crate) fn compute_font_bbox_y(font: &Font) -> Result<(f64, f64), BabelfontError> {
    let mut ymin = f64::INFINITY;
    let mut ymax = f64::NEG_INFINITY;
    for glyph in font.glyphs.iter() {
        for layer in glyph.layers.iter() {
            let bounds = layer.decomposed(font).bounds()?;
            ymin = ymin.min(bounds.min_y());
            ymax = ymax.max(bounds.max_y());
        }
    }
    if ymin > ymax {
        Ok((0.0, 0.0))
    } else {
        Ok((ymin, ymax))
    }
}

/// Compute the delta that should be written for an offset-mode metric.
///
/// When the original SFD stored a metric as a delta from FontForge's computed
/// default (the em ascent/descent for typo metrics, the font bounding box for
/// win/hhea metrics), the stored value = absolute_value - base.  This
/// function reconstructs that delta from the current (possibly user-modified)
/// absolute metric and the appropriate base.
pub(crate) fn compute_offset_delta(
    font: &Font,
    key: &str,
    absolute: i32,
) -> Result<i32, BabelfontError> {
    let Some(master) = font.masters.first() else {
        return Ok(absolute);
    };
    Ok(match key {
        "OS2TypoAscent" => {
            let base = master
                .metrics
                .get(&MetricType::Ascender)
                .copied()
                .unwrap_or(0);
            absolute - base
        }
        "OS2TypoDescent" => {
            let base = master
                .metrics
                .get(&MetricType::Descender)
                .copied()
                .unwrap_or(0);
            absolute - base
        }
        "OS2WinAscent" | "HheadAscent" => {
            let (_, ymax) = compute_font_bbox_y(font)?;
            absolute - ymax.round() as i32
        }
        "OS2WinDescent" => {
            let (ymin, _) = compute_font_bbox_y(font)?;
            absolute - (-ymin.round() as i32)
        }
        "HheadDescent" => {
            let (ymin, _) = compute_font_bbox_y(font)?;
            absolute - ymin.round() as i32
        }
        _ => absolute,
    })
}

#[allow(clippy::expect_used, clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use crate::convertors::fontforge::load_str;

    use super::*;

    #[test]
    fn test_offset_mode_vertical_metrics() {
        // The *AOffset/*DOffset flags mean the stored metric is a delta from
        // FontForge's computed default: em ascent/descent for typo, the font
        // bbox for win/hhea. The box glyph below spans y = -50 .. 780.
        let data = concat!(
            "SplineFontDB: 3.0\n",
            "Ascent: 800\n",
            "Descent: 200\n",
            "OS2TypoAscent: 0\n",
            "OS2TypoAOffset: 1\n",
            "OS2TypoDescent: 1\n",
            "OS2TypoDOffset: 1\n",
            "OS2WinAscent: 1\n",
            "OS2WinAOffset: 1\n",
            "OS2WinDescent: 0\n",
            "OS2WinDOffset: 1\n",
            "HheadAscent: 0\n",
            "HheadAOffset: 1\n",
            "HheadDescent: 1\n",
            "HheadDOffset: 1\n",
            "LayerCount: 2\n",
            "Layer: 0 0 \"Back\" 1\n",
            "Layer: 1 0 \"Fore\" 0\n",
            "BeginChars: 1 1\n",
            "StartChar: box\n",
            "Encoding: 65 65 0\n",
            "Width: 600\n",
            "Fore\n",
            "SplineSet\n",
            "100 -50 m 1\n",
            " 100 780 l 1\n",
            " 500 780 l 1\n",
            " 500 -50 l 1\n",
            " 100 -50 l 1\n",
            "EndSplineSet\n",
            "EndChar\n",
            "EndChars\n",
            "EndSplineFont\n"
        );
        let font = load_str(data).expect("Failed to parse offset-mode SFD");
        let m = |mt: MetricType| font.masters[0].metrics.get(&mt).copied();
        assert_eq!(m(MetricType::TypoAscender), Some(800)); // Ascent + 0
        assert_eq!(m(MetricType::TypoDescender), Some(-199)); // -Descent + 1
        assert_eq!(m(MetricType::WinAscent), Some(781)); // yMax + 1
        assert_eq!(m(MetricType::WinDescent), Some(50)); // -yMin + 0
        assert_eq!(m(MetricType::HheaAscender), Some(780)); // yMax + 0
        assert_eq!(m(MetricType::HheaDescender), Some(-49)); // yMin + 1
    }
}
