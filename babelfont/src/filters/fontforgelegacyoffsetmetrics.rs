use crate::filters::FontFilter;
use crate::{BabelfontError, Font, LayerType, MetricType, NodeType};

/// Where the FontForge reader keeps the delta an offset-mode metric states.
const OFFSET_DELTA_PREFIX: &str = "sfd.offset_delta.";
/// Where the FontForge reader marks a metric as resolved from a delta.
const OFFSET_MODE_PREFIX: &str = "sfd.offset_mode.";

/// The four offset-mode metrics whose base is the font's bounding box: the
/// metric's key, the key of its offset flag, and the metric it resolves to.
struct BoxMetric {
    key: &'static str,
    flag: &'static str,
    metric: MetricType,
}

static BOX_METRICS: [BoxMetric; 4] = [
    BoxMetric {
        key: "OS2WinAscent",
        flag: "OS2WinAOffset",
        metric: MetricType::WinAscent,
    },
    BoxMetric {
        key: "OS2WinDescent",
        flag: "OS2WinDOffset",
        metric: MetricType::WinDescent,
    },
    BoxMetric {
        key: "HheadAscent",
        flag: "HheadAOffset",
        metric: MetricType::HheaAscender,
    },
    BoxMetric {
        key: "HheadDescent",
        flag: "HheadDOffset",
        metric: MetricType::HheaDescender,
    },
];

/// An offset-mode metric and the delta the source states for it.
struct StatedDelta {
    metric: &'static BoxMetric,
    delta: i32,
}

/// The vertical extremes an on-curve-only exporter measures.
struct ExporterBounds {
    /// Lowest on-curve point of any glyph, floored (the `head` table's yMin).
    head_min: i32,
    /// Highest on-curve point of any glyph, ceiled (the `head` table's yMax).
    head_max: i32,
    /// Lowest point of any outline, truncated toward zero.
    outline_min: i32,
    /// Highest point of any outline, truncated toward zero.
    outline_max: i32,
}

#[derive(Default)]
/// A filter that resolves a FontForge source's offset-mode `usWinAscent`,
/// `usWinDescent`, `hhea.ascender` and `hhea.descender` against the bases of an
/// exporter that measures on-curve points only.
///
/// An offset-mode metric is the stated delta plus a base measured from the
/// outlines of the default master's decomposed glyphs:
///
/// - `head` box: per glyph, the floor of the lowest and the ceiling of the
///   highest ON-CURVE point. Control points do not count; the implied on-curve
///   points of a quadratic contour do.
/// - win metrics: `head.yMax` and `-head.yMin`.
/// - hhea metrics: the true outline extremes truncated toward zero, widened to
///   the `head` box, and to 0 when a glyph has no outline.
///
/// The reader's default base counts the true outline extremes, so the two
/// differ wherever a curve's on-curve points do not reach its extreme. A cubic
/// contour is measured by its own on-curve points, which is exact only when the
/// exported contour has the same on-curve points (a quadratic source).
///
/// It is opt-in: the right base is the one of the exporter that wrote the
/// binary being reproduced.
pub struct FontForgeLegacyOffsetMetrics;

impl FontForgeLegacyOffsetMetrics {
    /// Create a new FontForgeLegacyOffsetMetrics filter
    pub fn new() -> Self {
        FontForgeLegacyOffsetMetrics
    }
}

fn exporter_bounds(font: &Font) -> Result<Option<ExporterBounds>, BabelfontError> {
    let Some(master) = font.masters.first() else {
        return Ok(None);
    };
    let default_layer = LayerType::DefaultForMaster(master.id.clone());
    let mut on_curve_min = f64::INFINITY;
    let mut on_curve_max = f64::NEG_INFINITY;
    let mut outline_min = f64::INFINITY;
    let mut outline_max = f64::NEG_INFINITY;
    let mut has_empty_glyph = false;
    for glyph in font.glyphs.iter() {
        for layer in glyph
            .layers
            .iter()
            .filter(|layer| !layer.is_background && layer.master == default_layer)
        {
            let flat = layer.decomposed(font);
            let mut has_points = false;
            for node in flat
                .paths()
                .flat_map(|path| path.nodes.iter())
                .filter(|node| node.nodetype != NodeType::OffCurve)
            {
                on_curve_min = on_curve_min.min(node.y);
                on_curve_max = on_curve_max.max(node.y);
                has_points = true;
            }
            if !has_points {
                has_empty_glyph = true;
                continue;
            }
            let bounds = flat.bounds()?;
            outline_min = outline_min.min(bounds.min_y());
            outline_max = outline_max.max(bounds.max_y());
        }
    }
    if on_curve_min > on_curve_max {
        return Ok(None);
    }
    let head_min = on_curve_min.floor() as i32;
    let head_max = on_curve_max.ceil() as i32;
    let mut outline_min = outline_min.trunc() as i32;
    let mut outline_max = outline_max.trunc() as i32;
    if has_empty_glyph {
        outline_min = outline_min.min(0);
        outline_max = outline_max.max(0);
    }
    Ok(Some(ExporterBounds {
        head_min,
        head_max,
        outline_min,
        outline_max,
    }))
}

impl FontFilter for FontForgeLegacyOffsetMetrics {
    fn apply(&self, font: &mut Font) -> Result<(), BabelfontError> {
        let stated: Vec<StatedDelta> = BOX_METRICS
            .iter()
            .filter_map(|metric| {
                font.format_specific
                    .get(&format!("{}{}", OFFSET_DELTA_PREFIX, metric.key))
                    .and_then(|v| v.as_i64())
                    .map(|delta| StatedDelta {
                        metric,
                        delta: delta as i32,
                    })
            })
            .collect();
        if stated.is_empty() {
            return Ok(());
        }
        let Some(bounds) = exporter_bounds(font)? else {
            return Ok(());
        };
        let mut resolved = Vec::new();
        for StatedDelta { metric: m, delta } in stated {
            let base = match m.metric {
                MetricType::WinAscent => bounds.head_max,
                MetricType::WinDescent => -bounds.head_min,
                MetricType::HheaAscender => bounds.outline_max.max(bounds.head_max),
                _ => bounds.outline_min.min(bounds.head_min),
            };
            if let Some(master) = font.masters.first_mut() {
                master.metrics.insert(m.metric.clone(), base + delta);
            }
            // The value is now absolute; say so, so that writing the font back
            // as SFD does not re-apply a base to it.
            font.format_specific.remove(&format!("{}{}", OFFSET_MODE_PREFIX, m.key));
            font.format_specific.remove(&format!("{}{}", OFFSET_DELTA_PREFIX, m.key));
            font.format_specific.insert(
                m.flag.to_string(),
                serde_json::Value::String("0".to_string()),
            );
            resolved.push(format!("{} {}", m.key, base + delta));
        }
        log::info!(
            "Resolved offset-mode metrics against on-curve bases: {}",
            resolved.join(", ")
        );
        Ok(())
    }

    fn from_str(_s: &str) -> Result<Self, BabelfontError>
    where
        Self: Sized,
    {
        Ok(FontForgeLegacyOffsetMetrics::new())
    }

    #[cfg(feature = "cli")]
    fn arg() -> clap::Arg
    where
        Self: Sized,
    {
        clap::Arg::new("fontforgelegacyoffsetmetrics")
            .long("fontforge-legacy-offset-metrics")
            .help(
                "Resolve a FontForge source's offset-mode usWinAscent, usWinDescent and hhea \
                 ascender/descender against on-curve bases, as older FontForge exporters did: \
                 the win metrics from the head box, the floor/ceiling of each glyph's ON-CURVE \
                 points; the hhea metrics from the true outline extremes truncated toward zero \
                 and widened to that box",
            )
            .action(clap::ArgAction::SetTrue)
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(all(test, feature = "fontforge"))]
mod tests {
    use super::*;
    use crate::convertors::fontforge::load_str;

    // One quadratic glyph whose control points lie beyond its curves' extremes:
    // on-curve points span y = -20 .. 700, the curves reach -30.5 .. 730, and
    // the control points -41 .. 760.
    fn sfd(offsets: &str) -> String {
        format!(
            concat!(
                "SplineFontDB: 3.0\n",
                "Ascent: 800\n",
                "Descent: 200\n",
                "{}",
                "LayerCount: 2\n",
                "Layer: 0 1 \"Back\" 1\n",
                "Layer: 1 1 \"Fore\" 0\n",
                "BeginChars: 2 2\n",
                "StartChar: box\n",
                "Encoding: 65 65 0\n",
                "Width: 600\n",
                "Fore\n",
                "SplineSet\n",
                "100 -20 m 1\n",
                " 300 -41 300 -41 500 -20 c 0\n",
                " 500 700 l 1\n",
                " 300 760 300 760 100 700 c 0\n",
                " 100 -20 l 1\n",
                "EndSplineSet\n",
                "EndChar\n",
                "StartChar: space\n",
                "Encoding: 32 32 1\n",
                "Width: 300\n",
                "EndChar\n",
                "EndChars\n",
                "EndSplineFont\n"
            ),
            offsets
        )
    }

    fn metric(font: &Font, metric: MetricType) -> Option<i32> {
        font.masters[0].metrics.get(&metric).copied()
    }

    #[test]
    fn test_win_from_on_curve_box_hhea_from_truncated_outline() {
        let mut font = load_str(&sfd(concat!(
            "OS2WinAscent: 1\n",
            "OS2WinAOffset: 1\n",
            "OS2WinDescent: 0\n",
            "OS2WinDOffset: 1\n",
            "HheadAscent: 0\n",
            "HheadAOffset: 1\n",
            "HheadDescent: 1\n",
            "HheadDOffset: 1\n",
        )))
        .unwrap();
        FontForgeLegacyOffsetMetrics::new()
            .apply(&mut font)
            .unwrap();
        // head box -20 .. 700; outline -30.5 .. 730 truncates to -30 .. 730
        assert_eq!(metric(&font, MetricType::WinAscent), Some(701));
        assert_eq!(metric(&font, MetricType::WinDescent), Some(20));
        assert_eq!(metric(&font, MetricType::HheaAscender), Some(730));
        assert_eq!(metric(&font, MetricType::HheaDescender), Some(-29));
    }

    #[test]
    fn test_absolute_metrics_are_left_alone() {
        let mut font = load_str(&sfd(concat!(
            "OS2WinAscent: 900\n",
            "OS2WinAOffset: 0\n",
            "HheadDescent: -250\n",
            "HheadDOffset: 0\n",
        )))
        .unwrap();
        FontForgeLegacyOffsetMetrics::new()
            .apply(&mut font)
            .unwrap();
        assert_eq!(metric(&font, MetricType::WinAscent), Some(900));
        assert_eq!(metric(&font, MetricType::HheaDescender), Some(-250));
    }

    #[test]
    fn test_resolved_metric_is_written_back_as_absolute() {
        let mut font = load_str(&sfd("OS2WinAscent: 1\nOS2WinAOffset: 1\n")).unwrap();
        FontForgeLegacyOffsetMetrics::new()
            .apply(&mut font)
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.sfd");
        crate::convertors::fontforge::save_sfd(&font, &path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("OS2WinAscent: 701\n"), "{text}");
        assert!(text.contains("OS2WinAOffset: 0\n"), "{text}");
    }
}
