use glyphslib::{common::CustomParameter, Plist};

use crate::{convertors::glyphs3::get_cp, BabelfontError, Font, MetricType};

/// A set of paired functions to interpret/export font-level custom parameters
pub(crate) fn interpret_custom_parameters(font: &mut Font) -> Result<(), BabelfontError> {
    interpret_variable_font_origin(font)?;
    interpret_use_typo_metrics(font)?;
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

/// The OS/2 and `hhea` vertical metrics that Glyphs stores as *master-level*
/// custom parameters, not as entries in the `metrics` array. fontc (and Glyphs)
/// read these as custom parameters (master first, then font as a fallback), so
/// emitting them as metric slots — which the generic metric export otherwise
/// does — leaves them ignored and the built font falls back to computed bbox
/// defaults (wrong line height).
pub(crate) fn is_vertical_metric_cp(metric: &MetricType) -> bool {
    matches!(
        metric,
        MetricType::TypoAscender
            | MetricType::TypoDescender
            | MetricType::TypoLineGap
            | MetricType::WinAscent
            | MetricType::WinDescent
            | MetricType::HheaAscender
            | MetricType::HheaDescender
            | MetricType::HheaLineGap
    )
}

/// Emit a master's OS/2 + hhea vertical metrics (parsed into its metric map,
/// e.g. from a FontForge SFD's `OS2TypoAscent`/`OS2WinAscent`/`HheadAscent`
/// fields) as *master-level* custom parameters with values.
///
/// Parameters already present (typically restored verbatim from the master's
/// `format_specific` on a Glyphs→Glyphs round-trip) are left untouched.
pub(crate) fn append_master_vertical_metrics(
    custom_parameters: &mut Vec<CustomParameter>,
    master: &crate::Master,
) {
    // Fixed order so the emitted .glyphs is reproducible.
    let order = [
        MetricType::TypoAscender,
        MetricType::TypoDescender,
        MetricType::TypoLineGap,
        MetricType::WinAscent,
        MetricType::WinDescent,
        MetricType::HheaAscender,
        MetricType::HheaDescender,
        MetricType::HheaLineGap,
    ];
    for metric in order {
        if let Some(&value) = master.metrics.get(&metric) {
            if !custom_parameters
                .iter()
                .any(|cp| cp.name == metric.as_str())
            {
                custom_parameters.push(CustomParameter {
                    name: metric.as_str().to_string(),
                    value: Plist::Integer(value as i64),
                    disabled: false,
                });
            }
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
        use glyphslib::common::CustomParameter;
        use glyphslib::Plist;

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
        super::append_master_vertical_metrics(&mut light_cps, &light);
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
        super::append_master_vertical_metrics(&mut bold_cps, &bold);
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
        super::append_master_vertical_metrics(&mut preexisting, &light);
        assert_eq!(
            value(&preexisting, "typoAscender"),
            Some(Plist::Integer(999))
        );
        assert_eq!(value(&preexisting, "winAscent"), Some(Plist::Integer(1928)));
    }
}
