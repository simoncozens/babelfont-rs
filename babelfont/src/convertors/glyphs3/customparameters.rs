use glyphslib::{common::CustomParameter, Plist};

use crate::{
    common::FormatSpecific, convertors::glyphs3::get_cp, BabelfontError, Font, MetricType,
};

/// A set of paired functions to interpret/export font-level custom parameters
pub(crate) fn interpret_custom_parameters(font: &mut Font) -> Result<(), BabelfontError> {
    interpret_variable_font_origin(font)?;
    interpret_use_typo_metrics(font)?;
    interpret_vertical_metrics(font)?;
    Ok(())
}

pub(crate) fn export_font_level_cps(
    custom_parameters: &mut Vec<CustomParameter>,
    font: &mut Font,
) -> Result<(), BabelfontError> {
    export_variable_font_origin(custom_parameters, font)?;
    export_use_typo_metrics(custom_parameters, font)?;
    Ok(())
}

/// The OS/2 and `hhea` vertical metrics that Glyphs stores as custom
/// parameters (master-level, falling back to font-level), not as entries in
/// the `metrics` array. fontc (and Glyphs) read these as custom parameters,
/// so emitting them as metric slots — which the generic metric export
/// otherwise does — leaves them ignored and the built font falls back to
/// computed bbox defaults (wrong line height).
///
/// Fixed order so the emitted .glyphs is reproducible.
pub(crate) const VERTICAL_METRIC_TYPES: [MetricType; 8] = [
    MetricType::TypoAscender,
    MetricType::TypoDescender,
    MetricType::TypoLineGap,
    MetricType::WinAscent,
    MetricType::WinDescent,
    MetricType::HheaAscender,
    MetricType::HheaDescender,
    MetricType::HheaLineGap,
];

pub(crate) fn is_vertical_metric_cp(metric: &MetricType) -> bool {
    VERTICAL_METRIC_TYPES.contains(metric)
}

/// The enabled value of a custom parameter, unwrapped from the
/// `{value, disabled}` shape `copy_custom_parameters` stores it in; `None`
/// when the parameter is absent or disabled.
fn enabled_cp_value<'a>(
    format_specific: &'a FormatSpecific,
    name: &str,
) -> Option<&'a serde_json::Value> {
    let wrapper = get_cp(format_specific, name)?.as_object()?;
    if wrapper
        .get("disabled")
        .and_then(|d| d.as_bool())
        .unwrap_or(false)
    {
        return None;
    }
    wrapper.get("value")
}

/// A vertical-metric custom parameter's numeric value, or `None` when absent,
/// disabled, or non-numeric.
fn cp_metric_value(format_specific: &FormatSpecific, metric: &MetricType) -> Option<i32> {
    let value = enabled_cp_value(format_specific, metric.as_str())?;
    value
        .as_i64()
        .or_else(|| value.as_f64().map(|v| v.round() as i64))
        .map(|v| v as i32)
}

/// The load-side twin of [`append_master_vertical_metrics`]: read the OS/2 +
/// hhea vertical metrics a Glyphs source declares as custom parameters into
/// each master's metric map, a master-level parameter overriding a font-level
/// one (the resolution order Glyphs itself uses).
fn interpret_vertical_metrics(font: &mut Font) -> Result<(), BabelfontError> {
    let font_level: Vec<(MetricType, i32)> = VERTICAL_METRIC_TYPES
        .iter()
        .filter_map(|metric| {
            cp_metric_value(&font.format_specific, metric).map(|v| (metric.clone(), v))
        })
        .collect();
    for master in font.masters.iter_mut() {
        for metric in VERTICAL_METRIC_TYPES.iter() {
            if master.metrics.contains_key(metric) {
                continue;
            }
            let value = cp_metric_value(&master.format_specific, metric).or_else(|| {
                font_level
                    .iter()
                    .find(|(m, _)| m == metric)
                    .map(|(_, v)| *v)
            });
            if let Some(value) = value {
                master.metrics.insert(metric.clone(), value);
            }
        }
    }
    Ok(())
}

/// Emit a master's OS/2 + hhea vertical metrics (parsed into its metric map,
/// e.g. from a FontForge SFD's `OS2TypoAscent`/`OS2WinAscent`/`HheadAscent`
/// fields) as *master-level* custom parameters with values.
///
/// Parameters already present (typically restored verbatim from the master's
/// `format_specific` on a Glyphs→Glyphs round-trip) are left untouched, as is
/// any metric whose value a *font-level* parameter already carries — echoing
/// it onto every master would churn round-tripped files.
pub(crate) fn append_master_vertical_metrics(
    custom_parameters: &mut Vec<CustomParameter>,
    master: &crate::Master,
    font_format_specific: &FormatSpecific,
) {
    for metric in VERTICAL_METRIC_TYPES {
        if let Some(&value) = master.metrics.get(&metric) {
            if custom_parameters
                .iter()
                .any(|cp| cp.name == metric.as_str())
            {
                continue;
            }
            if cp_metric_value(font_format_specific, &metric) == Some(value) {
                continue;
            }
            custom_parameters.push(CustomParameter {
                name: metric.as_str().to_string(),
                value: Plist::Integer(value as i64),
                disabled: false,
            });
        }
    }
}

fn find_or_insert(cps: &mut Vec<CustomParameter>, cp: CustomParameter) {
    if let Some(existing) = cps.iter_mut().find(|existing| existing.name == cp.name) {
        *existing = cp;
    } else {
        cps.push(cp);
    }
}

fn interpret_variable_font_origin(font: &mut Font) -> Result<(), BabelfontError> {
    if let Some(origin_id) =
        get_cp(&font.format_specific, "Variable Font Origin").and_then(|x| x.as_str())
    {
        if let Some(origin_master) = font.masters.iter().find(|m| m.id == origin_id) {
            // Location of this master becomes the default value of each axis
            for axis in font.axes.iter_mut() {
                if let Some(loc) = origin_master.location.get(axis.tag) {
                    axis.default = Some(axis.designspace_to_userspace(loc)?);
                }
            }
        }
    }
    Ok(())
}

fn export_variable_font_origin(
    custom_parameters: &mut Vec<CustomParameter>,
    font: &Font,
) -> Result<(), BabelfontError> {
    if font.masters.len() < 2 {
        // Not a variable font
        return Ok(());
    }
    if font.axes.iter().any(|axis| axis.default != axis.min) {
        // Find the master that matches the default locations
        if let Some(master) = font.default_master() {
            find_or_insert(
                custom_parameters,
                CustomParameter {
                    name: "Variable Font Origin".to_string(),
                    value: master.id.clone().into(),
                    disabled: false,
                },
            );
        } else {
            return Err(BabelfontError::NoDefaultMaster);
        }
    }
    Ok(())
}

fn interpret_use_typo_metrics(font: &mut Font) -> Result<(), BabelfontError> {
    if let Some(use_typo_metrics_cp) = get_cp(&font.format_specific, "Use Typo Metrics") {
        let use_typo_metrics = use_typo_metrics_cp
            .as_object()
            .and_then(|v| v.get("value"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        let value = if use_typo_metrics != 0 { 1 << 7 } else { 0 };
        let current_value = font.custom_ot_values.os2_fs_selection.unwrap_or(0);
        font.custom_ot_values.os2_fs_selection = Some(current_value | value);
    }
    Ok(())
}

fn export_use_typo_metrics(
    custom_parameters: &mut Vec<CustomParameter>,
    font: &Font,
) -> Result<(), BabelfontError> {
    if let Some(fs_selection) = font.custom_ot_values.os2_fs_selection {
        let use_typo_metrics = (fs_selection & (1 << 7)) != 0;
        if use_typo_metrics {
            find_or_insert(
                custom_parameters,
                CustomParameter {
                    name: "Use Typo Metrics".to_string(),
                    value: use_typo_metrics.into(),
                    disabled: false,
                },
            );
        } else {
            // Ensure it's not present
            custom_parameters.retain(|cp| cp.name != "Use Typo Metrics");
        }
    }
    Ok(())
}

#[allow(clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use crate::convertors::glyphs3::as_glyphs3;
    use fontdrasil::{
        coords::{DesignCoord, Location},
        types::Tag,
    };

    #[test]
    fn test_set_typo_metrics() {
        let mut font = crate::load("resources/Nunito3.glyphs").unwrap();
        // Check default location is sane
        assert_eq!(
            font.default_location().unwrap(),
            Location::from(vec![
                (Tag::new(b"wght"), DesignCoord::new(42.0)),
                (Tag::new(b"ital"), DesignCoord::new(0.0)),
            ])
        );
        assert!(font.default_master().is_some());
        // Check that Use Typo Metrics is set
        assert!(font.custom_ot_values.os2_fs_selection.unwrap() & (1 << 7) != 0);
        // Clear it
        font.custom_ot_values.os2_fs_selection = Some(0);
        // Export it to glyphs
        let glyphs = as_glyphs3(&font).unwrap();
        // Check there's no Use Typo Metrics cp
        assert!(!glyphs
            .custom_parameters
            .iter()
            .any(|cp| cp.name == "Use Typo Metrics"));

        // Now set it again
        font.custom_ot_values.os2_fs_selection = Some(1 << 7);
        let glyphs = as_glyphs3(&font).unwrap();
        // Check there's a Use Typo Metrics cp
        assert!(glyphs
            .custom_parameters
            .iter()
            .any(|cp| cp.name == "Use Typo Metrics"));
    }

    #[test]
    fn test_export_vertical_metrics_as_master_level_cps() {
        use crate::{Master, MetricType};
        use glyphslib::{common::CustomParameter, Plist};

        // Two masters with different vertical metrics: each master's values
        // must land on that master, not at the font level.
        let mut light = Master::default();
        let mut bold = Master::default();
        for (m, lv, bv) in [
            (MetricType::TypoAscender, 1928, 1836),
            (MetricType::TypoDescender, -412, -724),
            (MetricType::WinAscent, 1928, 1836),
        ] {
            light.metrics.insert(m.clone(), lv);
            bold.metrics.insert(m, bv);
        }

        let value = |cps: &[CustomParameter], name: &str| {
            cps.iter().find(|c| c.name == name).map(|c| c.value.clone())
        };

        let mut light_cps = vec![];
        super::append_master_vertical_metrics(&mut light_cps, &light, &Default::default());
        assert_eq!(
            value(&light_cps, "typoAscender"),
            Some(Plist::Integer(1928))
        );
        assert_eq!(
            value(&light_cps, "typoDescender"),
            Some(Plist::Integer(-412))
        );
        assert_eq!(value(&light_cps, "winAscent"), Some(Plist::Integer(1928)));
        // A metric that was not set is not emitted.
        assert!(value(&light_cps, "typoLineGap").is_none());

        let mut bold_cps = vec![];
        super::append_master_vertical_metrics(&mut bold_cps, &bold, &Default::default());
        assert_eq!(value(&bold_cps, "typoAscender"), Some(Plist::Integer(1836)));
        assert_eq!(
            value(&bold_cps, "typoDescender"),
            Some(Plist::Integer(-724))
        );

        // A parameter already present (e.g. restored verbatim from a
        // Glyphs->Glyphs round-trip) is left untouched.
        let mut preexisting = vec![CustomParameter {
            name: "typoAscender".to_string(),
            value: Plist::Integer(999),
            disabled: false,
        }];
        super::append_master_vertical_metrics(&mut preexisting, &light, &Default::default());
        assert_eq!(
            value(&preexisting, "typoAscender"),
            Some(Plist::Integer(999))
        );
        assert_eq!(value(&preexisting, "winAscent"), Some(Plist::Integer(1928)));

        // A metric whose value a font-level parameter already carries is not
        // echoed onto the master; one that differs is.
        let mut font_fs = crate::common::FormatSpecific::default();
        font_fs.insert(
            format!(
                "{}typoAscender",
                crate::convertors::glyphs3::KEY_CUSTOM_PARAMETERS
            ),
            serde_json::json!({"value": 1928, "disabled": false}),
        );
        font_fs.insert(
            format!(
                "{}winAscent",
                crate::convertors::glyphs3::KEY_CUSTOM_PARAMETERS
            ),
            serde_json::json!({"value": 1000, "disabled": false}),
        );
        let mut covered = vec![];
        super::append_master_vertical_metrics(&mut covered, &light, &font_fs);
        assert!(value(&covered, "typoAscender").is_none());
        assert_eq!(value(&covered, "winAscent"), Some(Plist::Integer(1928)));
    }

    #[test]
    fn test_interpret_font_level_vertical_metrics() {
        use crate::MetricType;

        // Fustat declares typoAscender/hheaAscender/winAscent (and friends)
        // as *font-level* custom parameters.
        let font = crate::load("resources/Fustat.glyphs").unwrap();
        assert!(!font.masters.is_empty());
        for master in &font.masters {
            assert_eq!(master.metrics.get(&MetricType::TypoAscender), Some(&1000));
            assert_eq!(master.metrics.get(&MetricType::TypoDescender), Some(&-420));
            assert_eq!(master.metrics.get(&MetricType::HheaAscender), Some(&1000));
        }

        // On export the values stay covered by the font-level parameters:
        // no master-level copies appear, and no metric slots are added.
        let glyphs = as_glyphs3(&font).unwrap();
        assert!(glyphs
            .custom_parameters
            .iter()
            .any(|cp| cp.name == "typoAscender"));
        for master in &glyphs.masters {
            assert!(!master
                .custom_parameters
                .iter()
                .any(|cp| cp.name == "typoAscender"));
        }
        assert!(!glyphs.metrics.iter().any(|m| m.name == "typoAscender"));
    }

    #[test]
    fn test_interpret_master_level_vertical_metrics() {
        use crate::MetricType;

        // RadioCanadaDisplay declares its vertical metrics per master.
        let font = crate::load("resources/RadioCanadaDisplay.glyphs").unwrap();
        assert!(!font.masters.is_empty());
        for master in &font.masters {
            assert_eq!(master.metrics.get(&MetricType::TypoAscender), Some(&950));
            assert_eq!(master.metrics.get(&MetricType::HheaAscender), Some(&950));
        }

        // On export each master keeps exactly one copy (restored verbatim
        // from format_specific, not duplicated by the metric-map export).
        let glyphs = as_glyphs3(&font).unwrap();
        for master in &glyphs.masters {
            assert_eq!(
                master
                    .custom_parameters
                    .iter()
                    .filter(|cp| cp.name == "typoAscender")
                    .count(),
                1
            );
        }
    }

    #[test]
    fn test_disabled_vertical_metric_cp_not_interpreted() {
        use crate::MetricType;

        let mut fs = crate::common::FormatSpecific::default();
        fs.insert(
            format!(
                "{}typoAscender",
                crate::convertors::glyphs3::KEY_CUSTOM_PARAMETERS
            ),
            serde_json::json!({"value": 900, "disabled": true}),
        );
        assert_eq!(super::cp_metric_value(&fs, &MetricType::TypoAscender), None);
    }

    #[test]
    fn test_master_level_vertical_metric_overrides_font_level() {
        use crate::{Master, MetricType};

        let key = format!(
            "{}typoAscender",
            crate::convertors::glyphs3::KEY_CUSTOM_PARAMETERS
        );
        let mut font = crate::Font::default();
        font.format_specific.insert(
            key.clone(),
            serde_json::json!({"value": 1000, "disabled": false}),
        );
        let mut master = Master::default();
        master
            .format_specific
            .insert(key, serde_json::json!({"value": 950, "disabled": false}));
        font.masters.push(master);
        font.masters.push(Master::default());

        super::interpret_vertical_metrics(&mut font).unwrap();
        assert_eq!(
            font.masters[0].metrics.get(&MetricType::TypoAscender),
            Some(&950)
        );
        assert_eq!(
            font.masters[1].metrics.get(&MetricType::TypoAscender),
            Some(&1000)
        );
    }
}
