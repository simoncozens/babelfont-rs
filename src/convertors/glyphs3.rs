use crate::{Axis, BabelfontError, Font, GlyphList, Master};
use fontdrasil::coords::{DesignCoord, DesignLocation, UserCoord};
use glyphslib::glyphs3;
use std::{collections::HashMap, fs, path::PathBuf, str::FromStr};
use write_fonts::types::Tag;

pub fn load(path: PathBuf) -> Result<Font, BabelfontError> {
    log::debug!("Reading to string");
    let s = fs::read_to_string(&path).map_err(|source| BabelfontError::IO {
        path: path.clone(),
        source,
    })?;
    load_str(&s, path.clone())
}

pub fn load_str(s: &str, path: PathBuf) -> Result<Font, BabelfontError> {
    let mut font = Font::new();
    let glyphs_font =
        glyphslib::Font::load_str(s).map_err(|source| BabelfontError::PlistParse {
            source,
            path: path.clone(),
        })?;
    let glyphs_font = glyphs_font
        .as_glyphs3()
        .ok_or_else(|| BabelfontError::WrongConvertor { path })?;
    // Copy axes
    font.axes = glyphs_font
        .axes
        .iter()
        .map(|axis| Axis {
            tag: Tag::from_str(&axis.tag).unwrap_or_else(|_| Tag::new(b"????")),
            name: axis.name.clone().into(),
            hidden: axis.hidden,
            ..Default::default()
        })
        .collect();

    // Copy masters
    font.masters = glyphs_font
        .masters
        .iter()
        .map(|master| load_master(master, glyphs_font, &font))
        .collect();
    // Copy glyphs
    font.glyphs = GlyphList(glyphs_font.glyphs.iter().map(Into::into).collect());

    // Copy instances
    // Copy metadata
    font.names.family_name = glyphs_font.family_name.clone().into();
    // Copy kerning
    // Interpret metrics
    // Interpret axes
    interpret_axes(&mut font);

    Ok(font)
}

fn load_master(master: &glyphs3::Master, glyphs_font: &glyphs3::Glyphs3, font: &Font) -> Master {
    let designspace_to_location = |numbers: &[f64]| -> DesignLocation {
        numbers
            .iter()
            .zip(font.axes.iter())
            .map(|(number, axis)| (axis.tag, DesignCoord::new(*number)))
            .collect()
    };
    let f64_axes: Vec<f64> = master.axes_values.iter().map(|x| *x as f64).collect();
    let mut m = Master {
        name: master.name.clone().into(),
        id: master.id.clone(),
        location: designspace_to_location(&f64_axes),
        guides: master.guides.iter().map(Into::into).collect(),
        metrics: HashMap::new(),
        kerning: HashMap::new(),
        custom_ot_values: vec![],
    };
    m.kerning = glyphs_font
        .kerning
        .get(&m.id)
        .map(|kerndict| {
            let mut kerns = HashMap::new();
            for (first, items) in kerndict {
                for (second, kern) in items {
                    kerns.insert((first.clone(), second.clone()), *kern as i16);
                }
            }
            kerns
        })
        .unwrap_or_default();
    m
}

fn interpret_axes(font: &mut Font) {
    // This is going to look very wrong, but after much trial and error I can confirm
    // it works. First: load the axes assuming that userspace=designspace. Then
    // work out the axis mappings. Then apply the mappings to the axis locations.
    if let Some(origin) = font.masters.first() {
        // XXX *or* custom parameter Variable Font Origin
        for master in font.masters.iter() {
            for axis in font.axes.iter_mut() {
                let loc = master
                    .location
                    .get(axis.tag)
                    .unwrap_or(DesignCoord::default());
                axis.min = if axis.min.is_none() {
                    Some(UserCoord::new(loc.to_f64()))
                } else {
                    axis.min.map(|v| v.min(UserCoord::new(loc.to_f64())))
                };
                axis.max = if axis.max.is_none() {
                    Some(UserCoord::new(loc.to_f64()))
                } else {
                    axis.max.map(|v| v.max(UserCoord::new(loc.to_f64())))
                };
                if master.id == origin.id {
                    axis.default = Some(UserCoord::new(loc.to_f64()));
                }
            }
        }
        // XXX find axis mappings here

        for axis in font.axes.iter_mut() {
            axis.default = Some(
                axis.designspace_to_userspace(DesignCoord::new(
                    axis.default.map(|v| v.to_f64()).unwrap_or(0.0),
                ))
                .unwrap_or(UserCoord::default()),
            );
            axis.min = axis.min.map(|v| {
                axis.designspace_to_userspace(DesignCoord::new(v.to_f64()))
                    .unwrap_or(UserCoord::default())
            });
            axis.max = axis.max.map(|v| {
                axis.designspace_to_userspace(DesignCoord::new(v.to_f64()))
                    .unwrap_or(UserCoord::default())
            });
        }
    }
}

pub(crate) fn as_glyphs3(font: &Font) -> glyphs3::Glyphs3 {
    let axes = font
        .axes
        .iter()
        .map(|ax| glyphs3::Axis {
            hidden: false,
            name: ax.name(),
            tag: ax.tag.to_string(),
        })
        .collect();

    let mut our_metrics: Vec<crate::MetricType> = vec![];
    for master in font.masters.iter() {
        for key in master.metrics.keys() {
            if key.as_str().ends_with(" overshoot") {
                continue;
            }
            if !our_metrics.contains(key) {
                our_metrics.push(key.clone());
            }
        }
    }

    let glyphs_font = glyphs3::Glyphs3 {
        format_version: 3,
        family_name: font
            .names
            .family_name
            .get_default()
            .map(|x| x.to_string())
            .unwrap_or_default(),
        axes,
        metrics: our_metrics.iter().map(Into::into).collect(),
        masters: font
            .masters
            .iter()
            .map(|x| save_master(x, &font.axes, &our_metrics))
            .collect(),
        glyphs: font.glyphs.iter().map(Into::into).collect(),
        instances: font
            .instances
            .iter()
            .map(|x| save_instance(x, &font.axes))
            .collect(),
        date: font.date.format("%Y-%m-%d %H:%M:%S +0000").to_string(),
        keep_alternates_together: false,
        units_per_em: font.upm.into(),
        version: glyphslib::common::Version {
            major: font.version.0.into(),
            minor: font.version.1.into(),
        },
        ..Default::default() // Stuff we should probably get to one day
    };
    // Save kerning
    // Save custom parameters
    // Save metadata
    // Save features
    glyphs_font
}

fn save_master(master: &Master, axes: &[Axis], metrics: &[crate::MetricType]) -> glyphs3::Master {
    let mut axes_values = vec![];
    for axis in axes {
        axes_values.push(
            master
                .location
                .get(axis.tag)
                .map(|x| x.to_f64())
                .map(|x| x as f32)
                .unwrap_or(0.0),
        );
    }

    let mut metric_values: Vec<glyphs3::MetricValue> = vec![];
    for metric in metrics {
        let position = master.metrics.get(metric).copied().unwrap_or(0);
        let over = master
            .metrics
            .get(&crate::MetricType::Custom(format!(
                "{} overshoot",
                metric.as_str()
            )))
            .copied()
            .unwrap_or(0);
        metric_values.push(glyphs3::MetricValue {
            over: over as f32,
            pos: position as f32,
        });
    }

    glyphs3::Master {
        id: master.id.clone(),
        name: master
            .name
            .get_default()
            .map(|x| x.to_string())
            .unwrap_or_default(),
        axes_values,
        guides: master.guides.iter().map(Into::into).collect(),
        metric_values,
        ..Default::default()
    }
}

fn save_instance(instance: &crate::Instance, axes: &[Axis]) -> glyphs3::Instance {
    let mut axes_values = vec![];
    for axis in axes {
        axes_values.push(
            instance
                .location
                .get(axis.tag)
                .map(|x| x.to_f64())
                .map(|x| x as f32)
                .unwrap_or(0.0),
        );
    }
    glyphs3::Instance {
        name: instance
            .name
            .get_default()
            .map(|x| x.to_string())
            .unwrap_or_default(),
        axes_values,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use crate::Shape;

    use super::*;

    #[test]
    fn test_transform() {
        let f = load("resources/RadioCanadaDisplay.glyphs".into()).unwrap();
        let shape = f
            .glyphs
            .iter()
            .find(|g| g.name == "eacute")
            .unwrap()
            .layers
            .first()
            .unwrap()
            .shapes
            .get(1)
            .unwrap();
        if let Shape::ComponentShape(p) = shape {
            assert_eq!(p.reference, "acutecomb");
            assert_eq!(
                p.transform,
                kurbo::Affine::new([1.0, 0.0, 0.0, 1.0, 152.0, 0.0])
            );
        } else {
            panic!("Expected a component shape");
        }
    }

    #[test]
    fn test_roundtrip() {
        let there = load("resources/RadioCanadaDisplay.glyphs".into()).unwrap();
        let backagain = glyphslib::Font::Glyphs3(as_glyphs3(&there));
        fs::write(
            "resources/output/RadioCanadaDisplay.glyphs",
            backagain.to_string().unwrap(),
        )
        .unwrap();
    }
}
