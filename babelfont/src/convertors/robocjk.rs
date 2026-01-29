use fontdrasil::coords::{DesignCoord, DesignLocation, UserCoord};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use uuid::Uuid;

use crate::{
    common::tag_from_string,
    convertors::{
        fontra::Axes,
        robocjk::{
            axes::component_axes_from_lib, metrics::insert_metrics_from_layout,
            transform::parse_deep_component_transform,
        },
        ufo::{load_component, load_path},
    },
    error::BabelfontError,
    font::Font,
    layer::Layer,
    master::Master,
    LayerType, Shape,
};

mod axes;
mod metrics;
mod transform;
mod utils;

use metrics::LineMetricsHorizontalLayout;

/// A RoboCJK source definition
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RCJKSource {
    /// The name of the source
    pub name: String,
    /// The location of the source in design space coordinates
    pub location: HashMap<String, f64>,
    /// Horizontal layout metrics
    pub line_metrics_horizontal_layout: LineMetricsHorizontalLayout,
}

/// The RoboCJK designspace file structure
#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct DesignSpace {
    /// The axes definition
    pub axes: Axes,
    /// The sources definition (mapping source ID to source)
    pub sources: IndexMap<String, RCJKSource>,
}

/// Load deep components from a robocjk.deepComponents array into a layer
fn load_deep_components(layer: &mut Layer, deep_components_arr: &serde_json::Value) {
    use crate::Component;

    if let Some(arr) = deep_components_arr.as_array() {
        for item in arr {
            if let Some(name) = item.get("name").and_then(|v| v.as_str()) {
                let default_transform = serde_json::json!({});
                let transform_obj = item.get("transform").unwrap_or(&default_transform);
                let transform = parse_deep_component_transform(transform_obj);

                let mut component = Component {
                    reference: name.into(),
                    transform,
                    location: IndexMap::new(),
                    format_specific: Default::default(),
                };

                // Parse coord into component.location
                if let Some(coord) = item.get("coord").and_then(|v| v.as_object()) {
                    for (k, v) in coord.iter() {
                        if let Some(val) = v.as_f64() {
                            component.location.insert(k.clone(), DesignCoord::new(val));
                        }
                    }
                }

                layer.shapes.push(Shape::Component(component));
            }
        }
    }
}

const DEBUGGING: bool = false;

/// Sort a glyph's layers to match the order of masters in the font
fn sort_layers_by_master_order(glyph: &mut crate::Glyph, font: &Font) {
    // Create a map of master ID to position in the font's master list
    let master_order: std::collections::HashMap<&str, usize> = font
        .masters
        .iter()
        .enumerate()
        .map(|(idx, master)| (master.id.as_str(), idx))
        .collect();

    // Sort layers by master order
    glyph.layers.sort_by(|a, b| {
        let a_order = match &a.master {
            crate::LayerType::DefaultForMaster(id) => master_order.get(id.as_str()).copied(),
            _ => None,
        };
        let b_order = match &b.master {
            crate::LayerType::DefaultForMaster(id) => master_order.get(id.as_str()).copied(),
            _ => None,
        };

        // Layers associated with known masters come first, in master order
        match (a_order, b_order) {
            (Some(a_idx), Some(b_idx)) => a_idx.cmp(&b_idx),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
    });
}

/// Load character glyphs from the RCJK directory into the font
fn load_character_glyphs(font: &mut Font, base_path: &Path) -> Result<(), BabelfontError> {
    let glyph_dir = base_path.join("characterGlyph");
    if !glyph_dir.is_dir() {
        return Ok(());
    }

    // Identify the default master to attach layers to
    let default_master_id = font
        .default_master()
        .map(|m| m.id.clone())
        .unwrap_or_else(|| {
            font.masters
                .first()
                .map(|m| m.id.clone())
                .unwrap_or_default()
        });

    let mut glyph_paths: Vec<PathBuf> = std::fs::read_dir(&glyph_dir)
        .map_err(|e| BabelfontError::General(format!("Failed to read glyph dir: {}", e)))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("glif"))
        .collect();

    // Sort for deterministic ordering
    glyph_paths.sort();

    for path in glyph_paths {
        if DEBUGGING
            && !(path.file_name() == Some(std::ffi::OsStr::new("uni4E_01.glif"))
                || path.file_name() == Some(std::ffi::OsStr::new("V_G__4E_85_00.glif"))
                || path.file_name() == Some(std::ffi::OsStr::new("V_G__4E_00_00.glif")))
        {
            continue;
        }
        let mut bf_glyph =
            robocjk_glyph_to_babelfont_glyph(font, &glyph_dir, &default_master_id, path)?;

        // Sort layers to match master order
        sort_layers_by_master_order(&mut bf_glyph, font);

        font.glyphs.push(bf_glyph);
    }
    Ok(())
}

fn robocjk_glyph_to_babelfont_glyph(
    font: &mut Font,
    glyph_dir: &Path,
    default_master_id: &str,
    path: PathBuf,
) -> Result<crate::Glyph, BabelfontError> {
    let norad_glyph = norad::Glyph::load(&path).map_err(|e| {
        BabelfontError::General(format!("Failed to load glif {}: {}", path.display(), e))
    })?;
    let mut bf_glyph = crate::Glyph::new(norad_glyph.name().as_str());
    bf_glyph.codepoints = norad_glyph.codepoints.iter().map(|x| x as u32).collect();
    // Just export everything for now.
    // if !bf_glyph.codepoints.is_empty() {
    bf_glyph.exported = true;
    // }
    let lib_json = serde_json::to_value(&norad_glyph.lib).unwrap_or_default();
    let mut default_layer = layer_basics_from_norad_glyph(
        default_master_id,
        &norad_glyph,
        lib_json.get("robocjk.deepComponents"),
    );
    bf_glyph.component_axes = component_axes_from_lib(&norad_glyph.lib);

    ensure_smart_component_defaults(&mut default_layer, &bf_glyph);

    // Load variation glyph layers from robocjk.variationGlyphs
    let lib_json = serde_json::to_value(&norad_glyph.lib).unwrap_or_default();
    bf_glyph.layers.push(default_layer);
    if let Some(vars) = lib_json
        .get("robocjk.variationGlyphs")
        .and_then(|v| v.as_array())
    {
        load_variation_glyphs(
            font,
            &norad_glyph,
            glyph_dir,
            default_master_id,
            path,
            &mut bf_glyph,
            vars,
        );
    }

    Ok(bf_glyph)
}

fn load_variation_glyphs(
    font: &mut Font,
    base_glyph: &norad::Glyph,
    glyph_dir: &Path,
    default_master_id: &str,
    path: PathBuf,
    bf_glyph: &mut crate::Glyph,
    vars: &Vec<serde_json::Value>,
) {
    let file_name = path.file_name().map(|x| x.to_owned());
    let font_level_axis_tags = font
        .axes
        .iter()
        .map(|a| a.tag.to_string())
        .collect::<Vec<String>>();
    for item in vars {
        // println!("Processing variationGlyph item: {:?}", item);
        let layer_name = item
            .get("layerName")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let source_name = item.get("sourceName").and_then(|v| v.as_str());
        let location_obj = item.get("location").and_then(|v| v.as_object());
        if let (Some(source_name), Some(file_name)) = (source_name, file_name.as_ref()) {
            let var_path = glyph_dir.join(source_name).join(file_name);
            let var_glyph = norad::Glyph::load(&var_path).ok();
            let glyph_for_layer = var_glyph.as_ref().unwrap_or(base_glyph);

            let mut var_layer = layer_basics_from_norad_glyph(
                default_master_id,
                glyph_for_layer,
                item.get("deepComponents"),
            );
            var_layer.id = Some(Uuid::new_v4().to_string());
            var_layer.name = match layer_name {
                Some(name) if !name.is_empty() => Some(name),
                _ => Some(source_name.to_string()),
            };

            // Classify the location: glyph-level only vs. font-level axes
            if let Some(loc) = location_obj {
                let location_map: HashMap<String, f64> = loc
                    .iter()
                    .filter_map(|(k, v)| v.as_f64().map(|val| (k.clone(), val)))
                    .collect();

                if utils::is_glyph_level_only(&location_map, &*bf_glyph) {
                    // Smart component variation: associate with default master
                    var_layer.master =
                        LayerType::AssociatedWithMaster(default_master_id.to_string());

                    let mut scl: IndexMap<String, DesignCoord> = IndexMap::new();
                    for (k, v) in &location_map {
                        scl.insert(k.clone(), DesignCoord::new(*v));
                    }

                    var_layer.smart_component_location = scl;
                } else {
                    // Font-level axis variation: find master or create free-floating layer
                    if let Some(master) = utils::find_master_at_location(font, &location_map) {
                        var_layer.master = LayerType::DefaultForMaster(master.id.clone());
                    } else {
                        // No master found: create free-floating layer with location
                        var_layer.master = LayerType::FreeFloating;
                        let mut design_location = DesignLocation::new();
                        let mut scl = IndexMap::new();
                        for (axis_tag_str, value) in &location_map {
                            // If this is a font-level axis, insert into .location
                            if font_level_axis_tags.contains(axis_tag_str) {
                                #[allow(clippy::unwrap_used)]
                                // Already validated, we want to panic if this is wrong
                                design_location.insert(
                                    tag_from_string(axis_tag_str).unwrap(),
                                    DesignCoord::new(*value),
                                );
                            } else {
                                // Put it into the smart_component_location instead
                                scl.insert(axis_tag_str.clone(), DesignCoord::new(*value));
                            }
                        }
                        var_layer.location = Some(design_location);
                        var_layer.smart_component_location = scl;
                    }
                }
            }
            ensure_smart_component_defaults(&mut var_layer, &*bf_glyph);
            bf_glyph.layers.push(var_layer);
        }
    }
}

fn layer_basics_from_norad_glyph(
    default_master_id: &str,
    norad_glyph: &norad::Glyph,
    deep_components: Option<&serde_json::Value>,
) -> Layer {
    let mut layer = Layer::new(norad_glyph.width as f32);
    layer.id = Some(default_master_id.to_string());
    layer.master = LayerType::DefaultForMaster(default_master_id.to_string());
    for comp in &norad_glyph.components {
        layer.shapes.push(Shape::Component(load_component(comp)));
    }
    for contour in &norad_glyph.contours {
        layer.shapes.push(Shape::Path(load_path(contour)));
    }
    layer.guides = norad_glyph.guidelines.iter().map(|x| x.into()).collect();
    layer.anchors = norad_glyph.anchors.iter().map(|x| x.into()).collect();

    // Load deep components into layer
    if let Some(deep_comps) = deep_components {
        load_deep_components(&mut layer, deep_comps);
    }

    layer
}

fn ensure_smart_component_defaults(var_layer: &mut Layer, bf_glyph: &crate::Glyph) {
    // Ensure we have default coordinates for all component axes
    for axis in &bf_glyph.component_axes {
        if let Some(axis_name) = axis.name.get_default() {
            if !var_layer.smart_component_location.contains_key(axis_name) {
                let default_value = axis.default.unwrap_or(UserCoord::new(0.0)).to_f64();
                var_layer
                    .smart_component_location
                    .insert(axis_name.clone(), DesignCoord::new(default_value));
            }
        }
    }
}

/// Load a RoboCJK font from a directory
pub fn load(path: PathBuf) -> Result<Font, BabelfontError> {
    let designspace_path = path.join("designspace.json");
    let designspace_json = std::fs::read_to_string(&designspace_path)
        .map_err(|e| BabelfontError::General(format!("Failed to read designspace.json: {}", e)))?;

    let designspace: DesignSpace = serde_json::from_str(&designspace_json)
        .map_err(|e| BabelfontError::General(format!("Failed to parse designspace.json: {}", e)))?;

    let mut font = Font::new();

    // Convert axes
    font.axes = designspace
        .axes
        .axes
        .iter()
        .map(axes::axis_from_fontra)
        .collect::<Result<_, _>>()?;

    // Convert sources to masters
    for (source_id, rcjk_source) in &designspace.sources {
        let mut master = Master {
            name: rcjk_source.name.clone().into(),
            id: source_id.clone(),
            location: convert_location(&rcjk_source.location),
            ..Default::default()
        };

        insert_metrics_from_layout(&mut master, &rcjk_source.line_metrics_horizontal_layout);
        font.masters.push(master);
    }

    // Load glyphs from characterGlyph
    load_character_glyphs(&mut font, &path)?;

    Ok(font)
}

/// Convert a HashMap of axis values to a DesignLocation
fn convert_location(location: &HashMap<String, f64>) -> DesignLocation {
    let mut design_location = DesignLocation::new();
    for (axis_tag_str, value) in location {
        if let Ok(tag) = tag_from_string(axis_tag_str) {
            design_location.insert(tag, DesignCoord::new(*value));
        }
    }
    design_location
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use crate::MetricType;

    use super::*;
    use fontdrasil::coords::{DesignCoord, UserCoord};

    #[test]
    fn test_deserialize_designspace() {
        let designspace_json =
            std::fs::read_to_string("noto-cjk-varco/notosanscjksc.rcjk/designspace.json").unwrap();
        let designspace: DesignSpace = serde_json::from_str(&designspace_json).unwrap();

        // Check that we have axes
        assert_eq!(designspace.axes.axes.len(), 1);
        assert_eq!(designspace.axes.axes[0].name, "wght");
        assert_eq!(designspace.axes.axes[0].tag, "wght");
        assert_eq!(designspace.axes.axes[0].min_value, 100.0);
        assert_eq!(designspace.axes.axes[0].max_value, 900.0);
        assert_eq!(designspace.axes.axes[0].default_value, 100.0);

        // Check that we have sources
        assert_eq!(designspace.sources.len(), 2);
        assert!(designspace.sources.contains_key("53b46a1b"));
        assert!(designspace.sources.contains_key("c52203bb"));

        // Check a source
        let black_source = &designspace.sources["53b46a1b"];
        assert_eq!(black_source.name, "Black");
        assert_eq!(black_source.location.get("wght"), Some(&1.0));
        assert_eq!(
            black_source.line_metrics_horizontal_layout.ascender.value,
            800
        );
        assert_eq!(
            black_source.line_metrics_horizontal_layout.descender.value,
            -250
        );
    }

    #[test]
    fn test_load_robocjk() {
        let font = load("noto-cjk-varco/notosanscjksc.rcjk".into()).unwrap();

        // Check that axes were loaded
        assert_eq!(font.axes.len(), 1);
        let axis = &font.axes[0];
        assert_eq!(axis.tag.to_string(), "wght");
        assert_eq!(axis.min, Some(UserCoord::new(100.0)));
        assert_eq!(axis.max, Some(UserCoord::new(900.0)));
        assert_eq!(axis.default, Some(UserCoord::new(100.0)));
        assert!(!axis.hidden);

        // Check that axis mapping was loaded
        assert!(axis.map.is_some());
        let map = axis.map.as_ref().unwrap();
        assert_eq!(map.len(), 7);
        // Mapping values should be normalized coordinates (0.0 to 1.0)
        assert_eq!(map[0], (UserCoord::new(100.0), DesignCoord::new(0.0)));
        assert_eq!(map[6], (UserCoord::new(900.0), DesignCoord::new(1.0)));
    }

    #[test]
    fn test_load_robocjk_masters() {
        let font = load("noto-cjk-varco/notosanscjksc.rcjk".into()).unwrap();

        // Check that masters were loaded
        assert_eq!(font.masters.len(), 2);

        // Check the "Thin" master
        let thin_master = font
            .masters
            .iter()
            .find(|m| m.id == "c52203bb")
            .expect("Thin master not found");
        assert_eq!(
            thin_master.name.get_default().map(|s| s.as_str()),
            Some("Thin")
        );
        assert_eq!(
            thin_master.location.get(crate::Tag::new(b"wght")),
            Some(DesignCoord::new(0.0))
        );

        // Check metrics
        assert_eq!(
            thin_master.metrics.get(&MetricType::Ascender),
            Some(&800),
            "Ascender metric not loaded"
        );
        assert_eq!(
            thin_master.metrics.get(&MetricType::CapHeight),
            Some(&750),
            "Cap height metric not loaded"
        );
        assert_eq!(
            thin_master.metrics.get(&MetricType::XHeight),
            Some(&500),
            "X-height metric not loaded"
        );
        assert_eq!(
            thin_master.metrics.get(&MetricType::Descender),
            Some(&-250),
            "Descender metric not loaded"
        );

        // Check the "Black" master
        let black_master = font
            .masters
            .iter()
            .find(|m| m.id == "53b46a1b")
            .expect("Black master not found");
        assert_eq!(
            black_master.name.get_default().map(|s| s.as_str()),
            Some("Black")
        );
        assert_eq!(
            black_master.location.get(crate::Tag::new(b"wght")),
            Some(DesignCoord::new(1.0))
        );

        // Check metrics are the same (they are in the test data)
        assert_eq!(black_master.metrics.get(&MetricType::Ascender), Some(&800));
    }

    #[test]
    fn test_load_robocjk_glyphs() {
        let font = load("noto-cjk-varco/notosanscjksc.rcjk".into()).unwrap();

        // Check that a known glyph is loaded
        let glyph = font.glyphs.get("VG_31C0_00").expect("Glyph not loaded");
        assert!(!glyph.layers.is_empty(), "Glyph has no layers");
        assert!(
            !glyph.layers[0].shapes.is_empty(),
            "Default layer has no shapes"
        );

        // Check component axes loaded from robocjk.axes
        assert!(!glyph.component_axes.is_empty(), "No component axes loaded");
        let axis_names: Vec<String> = glyph
            .component_axes
            .iter()
            .map(|a| a.name.get_default().unwrap_or(&"".to_string()).to_string())
            .collect();
        assert!(axis_names.contains(&"weight".to_string()));
        assert!(axis_names.contains(&"width".to_string()));
    }

    #[test]
    fn test_load_robocjk_codepoints() {
        let font = load("noto-cjk-varco/notosanscjksc.rcjk".into()).unwrap();

        // Assert that at least one glyph has U+4E00 assigned
        let has_4e00 = font.glyphs.iter().any(|g| g.codepoints.contains(&0x4E00));
        assert!(has_4e00, "No glyph with U+4E00 found");

        // Sanity: many glyphs should have codepoints
        let count_with_codepoints = font
            .glyphs
            .iter()
            .filter(|g| !g.codepoints.is_empty())
            .count();
        assert!(
            count_with_codepoints > 100,
            "Unexpectedly few glyphs have codepoints"
        );
    }

    #[test]
    fn test_load_robocjk_variation_layer_classification() {
        let font = load("noto-cjk-varco/notosanscjksc.rcjk".into()).unwrap();

        // Check that glyphs with component axes have variation layers
        let glyph = font
            .glyphs
            .iter()
            .find(|g| !g.component_axes.is_empty())
            .expect("No glyph with component axes found");

        // Should have multiple layers
        assert!(
            glyph.layers.len() > 1,
            "Glyph with component axes should have variation layers"
        );

        // Verify that some layers are classified correctly:
        // - If a layer has smart_component_location but no explicit location, it's glyph-level
        // - If a layer has an explicit location and DefaultForMaster or FreeFloating, it's font-level
        let has_smart_location_layer = glyph
            .layers
            .iter()
            .any(|layer| !layer.smart_component_location.is_empty() && layer.location.is_none());
        assert!(
            has_smart_location_layer,
            "Expected at least one glyph-level axis variation layer"
        );
    }

    #[test]
    fn test_load_robocjk_deep_components() {
        let font = load("noto-cjk-varco/notosanscjksc.rcjk".into()).unwrap();

        // Find a glyph with deep components (T_2FF0_4E01 from the test file)
        let glyph = font
            .glyphs
            .iter()
            .find(|g| g.name.contains("2FF0_4E01"))
            .expect("Expected to find glyph with deep components");

        // Check that the default layer has deep components
        let default_layer = glyph.layers.first().expect("No default layer");
        let deep_components: Vec<_> = default_layer
            .shapes
            .iter()
            .filter_map(|s| {
                if let crate::Shape::Component(c) = s {
                    if !c.location.is_empty() {
                        Some(c)
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();

        assert!(
            !deep_components.is_empty(),
            "Expected deep components in default layer"
        );

        // Verify that at least one deep component has coordinate data
        let has_coord_data = deep_components.iter().any(|c| !c.location.is_empty());
        assert!(
            has_coord_data,
            "Expected deep components to have coordinate data"
        );
    }

    #[test]
    fn test_load_robocjk_variation_inlined_in_base_glif() {
        let font = load("noto-cjk-varco/notosanscjksc.rcjk".into()).unwrap();

        let glyph = font
            .glyphs
            .get("uni4E09")
            .expect("Expected uni4E09 glyph to load");

        let default_master_id = font
            .default_master()
            .map(|m| m.id.clone())
            .expect("Font should have a default master");

        let mut variation_layers = glyph
            .layers
            .iter()
            .filter(|layer| layer.id.as_ref() != Some(&default_master_id));

        let variation_layer = variation_layers
            .next()
            .expect("Expected an inlined variation layer for uni4E09");

        // Verify deep component coordinates were loaded from robocjk.variationGlyphs
        let has_heavy_weight_component = variation_layer.shapes.iter().any(|shape| {
            if let Shape::Component(c) = shape {
                if let Some(weight) = c.location.get("weight") {
                    return (weight.to_f64() - 149.0).abs() < f64::EPSILON;
                }
            }
            false
        });

        assert!(
            has_heavy_weight_component,
            "Variation layer should include deep component coords from robocjk.variationGlyphs"
        );
    }
}
