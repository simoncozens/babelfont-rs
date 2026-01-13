use glyphslib::common::CustomParameter;

use crate::{convertors::glyphs3::get_cp, BabelfontError, Font};

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
    use fontdrasil::coords::{DesignCoord, Location};
    use fontdrasil::types::Tag;

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
}
