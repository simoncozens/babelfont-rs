use crate::{master::Master, metrics::MetricType};
use serde::{Deserialize, Serialize};

/// Horizontal layout metrics for a source
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LineMetricsHorizontalLayout {
    /// The ascender metric
    pub ascender: MetricValue,
    /// The baseline metric
    pub baseline: MetricValue,
    /// The cap height metric
    pub cap_height: MetricValue,
    /// The descender metric
    pub descender: MetricValue,
    /// The x-height metric
    pub x_height: MetricValue,
}

/// A metric value with an associated zone
#[derive(Serialize, Deserialize, Debug, Clone)]
pub(crate) struct MetricValue {
    /// The metric value
    pub value: i32,
    /// The zone associated with the metric
    pub zone: i32,
}

/// Insert metrics from LineMetricsHorizontalLayout into a Master
pub(crate) fn insert_metrics_from_layout(
    master: &mut Master,
    layout: &LineMetricsHorizontalLayout,
) {
    // Insert ascender value and zone
    master
        .metrics
        .insert(MetricType::Ascender, layout.ascender.value);
    if layout.ascender.zone != 0 {
        master.metrics.insert(
            MetricType::Custom("ascender zone".to_string()),
            layout.ascender.zone,
        );
    }

    // Insert baseline value and zone
    if layout.baseline.value != 0 {
        master.metrics.insert(
            MetricType::Custom("baseline".to_string()),
            layout.baseline.value,
        );
    }
    if layout.baseline.zone != 0 {
        master.metrics.insert(
            MetricType::Custom("baseline zone".to_string()),
            layout.baseline.zone,
        );
    }

    // Insert cap height value and zone
    master
        .metrics
        .insert(MetricType::CapHeight, layout.cap_height.value);
    if layout.cap_height.zone != 0 {
        master.metrics.insert(
            MetricType::Custom("cap height zone".to_string()),
            layout.cap_height.zone,
        );
    }

    // Insert descender value and zone
    master
        .metrics
        .insert(MetricType::Descender, layout.descender.value);
    if layout.descender.zone != 0 {
        master.metrics.insert(
            MetricType::Custom("descender zone".to_string()),
            layout.descender.zone,
        );
    }

    // Insert x-height value and zone
    master
        .metrics
        .insert(MetricType::XHeight, layout.x_height.value);
    if layout.x_height.zone != 0 {
        master.metrics.insert(
            MetricType::Custom("x-height zone".to_string()),
            layout.x_height.zone,
        );
    }
}
