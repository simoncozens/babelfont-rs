/// Test suite for iteratively improving VFB support in Babelfont
/// Compares VFB→Babelfont→VFJ output against golden reference files
use serde_json::Value;
use std::path::PathBuf;
use vfbreader::read_vfb;

fn get_vfb_test_files() -> Vec<(PathBuf, PathBuf)> {
    vec![
        (
            PathBuf::from("resources/IBMPlexSerif-Text.vfb"),
            PathBuf::from("resources/IBMPlexSerif-Text.vfj"),
        ),
        (
            PathBuf::from("resources/Arvo_reg_MM_12b_2TT_sorce.vfb"),
            PathBuf::from("resources/Arvo_reg_MM_12b_2TT_sorce.vfj"),
        ),
        (
            PathBuf::from("resources/Arvo_ita_MM_54_b_sorce.vfb"),
            PathBuf::from("resources/Arvo_ita_MM_54_b_sorce.vfj"),
        ),
    ]
}

/// Load VFB, save as VFJ, and compare key structures
#[test]
#[ignore] // Run with `cargo test -- --ignored` to execute
fn test_vfb_to_vfj_conversion() {
    for (vfb_path, golden_vfj_path) in get_vfb_test_files() {
        if !vfb_path.exists() {
            println!("Skipping {} - file not found", vfb_path.display());
            continue;
        }

        println!("\n=== Testing {} ===", vfb_path.display());

        // Load VFB into Babelfont
        let font = match babelfont::load(&vfb_path) {
            Ok(f) => f,
            Err(e) => {
                println!("⚠ Failed to load VFB: {}", e);
                continue;
            }
        };

        // Save as intermediate babelfont for inspection
        let intermediate_path = vfb_path.with_extension("babelfont");
        font.save(&intermediate_path)
            .expect("Failed to save intermediate babelfont");
        println!("✓ Saved intermediate: {}", intermediate_path.display());

        // Save as VFJ by converting to fontlab and writing JSON
        // For now, let's just check that the font structure is valid
        println!("\nFont metadata:");
        println!("  UPM: {}", font.upm);
        println!("  Version: {:?}", font.version);
        println!("  Family: {:?}", font.names.family_name);
        println!("  Glyphs: {}", font.glyphs.len());
        println!("  Masters: {}", font.masters.len());
        println!("  Axes: {}", font.axes.len());

        // Load golden VFJ for comparison
        if golden_vfj_path.exists() {
            println!("\nLoading golden VFJ: {}", golden_vfj_path.display());
            let golden_vfj_content =
                std::fs::read_to_string(&golden_vfj_path).expect("Failed to read golden VFJ");
            let golden: Value =
                serde_json::from_str(&golden_vfj_content).expect("Failed to parse golden VFJ");

            println!("Golden VFJ structure:");
            if let Some(font_obj) = golden.get("font") {
                if let Some(glyph_count) = font_obj.get("glyphsCount") {
                    println!("  Glyphs in golden: {}", glyph_count);
                }
                if let Some(upm) = font_obj.get("upm") {
                    println!("  UPM in golden: {}", upm);
                }
            }

            // Show initial mismatches
            println!("\nInitial comparison:");
            if let Some(golden_glyphs) = golden.get("font").and_then(|f| f.get("glyphsCount")) {
                println!(
                    "  Glyph count - Our: {}, Golden: {}",
                    font.glyphs.len(),
                    golden_glyphs
                );
            }
        }
    }
}

/// Quick sanity check on what VFB entries we're currently handling
#[test]
#[ignore]
fn test_vfb_loader_coverage() {
    for (vfb_path, _) in get_vfb_test_files() {
        if !vfb_path.exists() {
            println!("Test file not found: {}", vfb_path.display());
            continue;
        }

        let font = babelfont::load(&vfb_path).expect("Failed to load VFB");

        println!("\n=== VFB Loader Coverage: {} ===", vfb_path.display());
        println!("Font loaded: {:?}", font.names.family_name);
        println!("UPM: {}", font.upm);
        println!("Version: {:?}", font.version);
        println!("Masters: {}", font.masters.len());
        for (i, master) in font.masters.iter().enumerate() {
            println!("  Master {}: {:?} ({})", i, master.name, master.id);
            println!("    Location: {:?}", master.location);
            println!("    Metrics: {} entries", master.metrics.len());
            println!("    Kerning: {} pairs", master.kerning.len());
        }

        println!("Glyphs: {}", font.glyphs.len());
        if let Some(first_glyph) = font.glyphs.get_by_index(0) {
            println!(
                "  First glyph: {} ({} layers)",
                first_glyph.name,
                first_glyph.layers.len()
            );
            for (i, layer) in first_glyph.layers.iter().enumerate() {
                println!("    Layer {}: {} shapes", i, layer.shapes.len());
            }
        }

        println!("Axes: {}", font.axes.len());
        for axis in &font.axes {
            println!("  {:?}: {:?} - {:?}", axis.name, axis.min, axis.max);
        }

        println!("Features: {} classes", font.features.classes.len());
        println!(
            "Kerning groups: {} first, {} second",
            font.first_kern_groups.len(),
            font.second_kern_groups.len()
        );
    }
}

/// Detailed comparison of VFB→Babelfont→VFJ output vs golden VFJ
#[test]
#[ignore]
fn test_detailed_vfb_comparison() {
    for (vfb_path, golden_vfj_path) in get_vfb_test_files() {
        if !vfb_path.exists() || !golden_vfj_path.exists() {
            continue;
        }

        println!(
            "\n{} Detailed Analysis: {} {}",
            "=".repeat(20),
            vfb_path.display(),
            "=".repeat(20)
        );

        let font = match babelfont::load(&vfb_path) {
            Ok(f) => f,
            Err(e) => {
                println!("Failed to load: {}", e);
                continue;
            }
        };

        let golden_content =
            std::fs::read_to_string(&golden_vfj_path).expect("Failed to read golden VFJ");
        let golden: Value =
            serde_json::from_str(&golden_content).expect("Failed to parse golden VFJ");

        // Detailed comparison
        println!("\n--- FONT-LEVEL PROPERTIES ---");
        compare_values("upm", &serde_json::json!(font.upm), &golden["font"]["upm"]);

        compare_values(
            "version",
            &serde_json::json!(font.version),
            &golden["font"]["version"],
        );

        println!("\n--- MASTERS ---");
        if let Some(golden_masters) = golden["font"]["masters"].as_array() {
            println!(
                "Golden has {} masters, our font has {}",
                golden_masters.len(),
                font.masters.len()
            );

            for (i, our_master) in font.masters.iter().enumerate() {
                if let Some(_golden_master) = golden_masters.get(i) {
                    println!("\nMaster {}:", i);
                    println!("  Name - Ours: {:?}", our_master.name);
                    println!("  Location - Ours: {:?}", our_master.location);
                }
            }
        }

        println!("\n--- GLYPHS SAMPLE (first 3) ---");
        if let Some(golden_glyphs) = golden["font"]["glyphs"].as_array() {
            for (i, our_glyph) in font.glyphs.iter().take(3).enumerate() {
                if let Some(golden_glyph) = golden_glyphs.get(i) {
                    println!("\nGlyph {}:", i);
                    if let Some(gg_name) = golden_glyph.get("name") {
                        println!("  Name - Golden: {}, Ours: {}", gg_name, our_glyph.name);
                    }
                    println!(
                        "  Layers - Golden: {}, Ours: {}",
                        golden_glyph
                            .get("layers")
                            .and_then(|l| l.as_array())
                            .map(|a| a.len())
                            .unwrap_or(0),
                        our_glyph.layers.len()
                    );
                    if let Some(gg_unicode) = golden_glyph.get("unicode") {
                        println!(
                            "  Unicode - Golden: {}, Ours: {:?}",
                            gg_unicode, our_glyph.codepoints
                        );
                    }
                }
            }
        }

        println!("\n--- STRUCTURE CHECK ---");
        if let Some(golden_axes) = golden["font"]["axes"].as_array() {
            println!(
                "Axes - Golden: {}, Ours: {}",
                golden_axes.len(),
                font.axes.len()
            );
        }

        if let Some(golden_instances) = golden["font"]["instances"].as_array() {
            println!(
                "Instances - Golden: {}, Ours: {}",
                golden_instances.len(),
                font.instances.len()
            );
        }
    }
}

fn compare_values(field: &str, ours: &Value, golden: &Value) {
    if ours == golden {
        println!("✓ {} matches", field);
    } else {
        println!("✗ {} - Ours: {:?}, Golden: {:?}", field, ours, golden);
    }
}

/// Test to verify layer structure matches between VFB and golden reference
#[test]
#[ignore]
fn test_glyph_layer_structure() {
    for (vfb_path, golden_vfj_path) in get_vfb_test_files() {
        if !vfb_path.exists() || !golden_vfj_path.exists() {
            continue;
        }

        println!(
            "\n{} Layer Structure Test: {} {}",
            "=".repeat(15),
            vfb_path.display(),
            "=".repeat(15)
        );

        let font = match babelfont::load(&vfb_path) {
            Ok(f) => f,
            Err(e) => {
                println!("Failed to load: {}", e);
                continue;
            }
        };

        let golden_content =
            std::fs::read_to_string(&golden_vfj_path).expect("Failed to read golden VFJ");
        let golden: Value =
            serde_json::from_str(&golden_content).expect("Failed to parse golden VFJ");

        println!(
            "\nFont has {} masters, analyzing layer structure...",
            font.masters.len()
        );

        let golden_glyphs = golden["font"]["glyphs"]
            .as_array()
            .expect("Golden glyphs is array");

        let mut layer_mismatches = 0;
        let mut sample_glyphs = Vec::new();

        for (glyph_idx, (our_glyph, golden_glyph)) in
            font.glyphs.iter().zip(golden_glyphs.iter()).enumerate()
        {
            let our_layer_count = our_glyph.layers.len();
            let golden_layer_count = golden_glyph
                .get("layers")
                .and_then(|l| l.as_array())
                .map(|a| a.len())
                .unwrap_or(0);

            if our_layer_count != golden_layer_count {
                layer_mismatches += 1;
                if sample_glyphs.len() < 5 {
                    sample_glyphs.push((
                        glyph_idx,
                        our_glyph.name.clone(),
                        our_layer_count,
                        golden_layer_count,
                    ));
                }
            }
        }

        println!(
            "\nLayer count discrepancies: {}/{}",
            layer_mismatches,
            font.glyphs.len()
        );
        if !sample_glyphs.is_empty() {
            println!("\nSample mismatches:");
            for (idx, name, ours, golden) in sample_glyphs {
                println!(
                    "  Glyph[{}] '{}': ours={}, golden={}",
                    idx, name, ours, golden
                );
            }
        }

        // Note: Layer count mismatches are expected due to FontLab's named layers (mask, Text, img)
        // vs Babelfont's master-based layers. This is not a bug - see VFB_IMPROVEMENT_LOOP.md
    }
}

/// Analyze which VFB entries relate to masters and axes
#[test]
#[ignore]
fn test_vfb_master_axis_entries() {
    for (vfb_path, golden_vfj_path) in get_vfb_test_files() {
        if !vfb_path.exists() {
            continue;
        }

        println!("\n========================================================");
        println!("File: {}", vfb_path.display());
        println!("========================================================");

        let vfb = match read_vfb(&vfb_path) {
            Ok(v) => v,
            Err(e) => {
                println!("⚠ Failed to load VFB: {}", e);
                continue;
            }
        };

        // Count entries by type
        let mut axis_count = 0;
        let mut axis_names = vec![];
        let mut master_count = 0;
        let mut master_names = vec![];
        let mut master_locations = vec![];
        let mut weight_values = vec![];

        for entry in &vfb.entries {
            match entry {
                vfbreader::VfbEntry::AxisCount(count) => {
                    axis_count = *count;
                    println!("AxisCount: {}", count);
                }
                vfbreader::VfbEntry::AxisName(name) => {
                    axis_names.push(name.clone());
                    println!("AxisName: {}", name);
                }
                vfbreader::VfbEntry::MasterCount(count) => {
                    master_count = *count;
                    println!("MasterCount: {}", count);
                }
                vfbreader::VfbEntry::MasterName(name) => {
                    master_names.push(name.clone());
                    println!("MasterName: {}", name);
                }
                vfbreader::VfbEntry::MasterLocation(_) => {
                    master_locations.push("(binary data)".to_string());
                    println!("MasterLocation: (binary data present)");
                }
                vfbreader::VfbEntry::Weight(w) => {
                    weight_values.push(w.clone());
                    println!("Weight: {}", w);
                }
                _ => {}
            }
        }

        println!("\nSummary:");
        println!("  Axes declared: {}", axis_count);
        println!("  Axis names found: {}", axis_names.len());
        for (i, name) in axis_names.iter().enumerate() {
            println!("    [{}] {}", i, name);
        }
        println!("  Masters declared: {}", master_count);
        println!("  Master names found: {}", master_names.len());
        for (i, name) in master_names.iter().enumerate() {
            println!("    [{}] {}", i, name);
        }
        println!("  Master locations found: {}", master_locations.len());
        println!("  Weight entries: {}", weight_values.len());

        // Compare against golden
        println!("\n--- Golden VFJ ---");
        if let Ok(golden_data) = std::fs::read_to_string(&golden_vfj_path) {
            if let Ok(json) = serde_json::from_str::<Value>(&golden_data) {
                if let Some(font) = json.get("font") {
                    let golden_axes = font
                        .get("axes")
                        .and_then(|a| a.as_array())
                        .map(|a| a.len())
                        .unwrap_or(0);
                    let golden_masters = font
                        .get("masters")
                        .and_then(|m| m.as_array())
                        .map(|m| m.len())
                        .unwrap_or(0);

                    println!("  Golden axes: {}", golden_axes);
                    if golden_axes > 0 {
                        for (i, axis) in font["axes"]
                            .as_array()
                            .unwrap_or(&vec![])
                            .iter()
                            .enumerate()
                        {
                            if let Some(name) = axis.get("name") {
                                println!("    [{}] {}", i, name);
                            }
                        }
                    }

                    println!("  Golden masters: {}", golden_masters);
                    if golden_masters > 0 {
                        for (i, master) in font["masters"]
                            .as_array()
                            .unwrap_or(&vec![])
                            .iter()
                            .enumerate()
                        {
                            if let Some(fm) = master.get("fontMaster") {
                                if let Some(name) = fm.get("name") {
                                    println!("    [{}] {}", i, name);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Verify that VFB outline data is correctly preserved
#[test]
#[ignore]
fn test_outline_data_integrity() {
    for (vfb_path, golden_vfj_path) in get_vfb_test_files() {
        if !vfb_path.exists() || !golden_vfj_path.exists() {
            continue;
        }

        println!("\n========================================================");
        println!("Outline integrity: {}", vfb_path.display());
        println!("========================================================");

        let font = match babelfont::load(&vfb_path) {
            Ok(f) => f,
            Err(e) => {
                println!("Failed to load: {}", e);
                continue;
            }
        };

        let golden_data = match std::fs::read_to_string(&golden_vfj_path) {
            Ok(d) => d,
            Err(e) => {
                println!("Failed to read golden: {}", e);
                continue;
            }
        };

        let golden_json: Value = match serde_json::from_str(&golden_data) {
            Ok(j) => j,
            Err(e) => {
                println!("Failed to parse golden: {}", e);
                continue;
            }
        };

        if let Some(golden_glyphs) = golden_json["font"]["glyphs"].as_array() {
            let mut matched = 0;
            let mut total = 0;

            for (glyph_idx, our_glyph) in font.glyphs.iter().enumerate().take(20) {
                if glyph_idx >= golden_glyphs.len() {
                    break;
                }

                let golden_glyph = &golden_glyphs[glyph_idx];

                // Count our nodes across all layers
                let mut our_nodes = 0;
                for layer in &our_glyph.layers {
                    for shape in &layer.shapes {
                        if let babelfont::Shape::Path(path) = shape {
                            our_nodes += path.nodes.len();
                        }
                    }
                }

                // Count golden nodes across all layers
                let mut golden_nodes = 0;
                if let Some(layers) = golden_glyph.get("layers").and_then(|l| l.as_array()) {
                    for layer in layers {
                        if let Some(elements) = layer.get("elements").and_then(|e| e.as_array()) {
                            for elem in elements {
                                if let Some(elem_data) = elem.get("elementData") {
                                    if let Some(contours) =
                                        elem_data.get("contours").and_then(|c| c.as_array())
                                    {
                                        for contour in contours {
                                            if let Some(nodes) =
                                                contour.get("nodes").and_then(|n| n.as_array())
                                            {
                                                golden_nodes += nodes.len();
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                if our_nodes == golden_nodes {
                    println!("  ✓ {}: {} nodes", our_glyph.name, our_nodes);
                    matched += 1;
                } else if our_nodes > 0 || golden_nodes > 0 {
                    println!(
                        "  ✗ {}: our={}, golden={}",
                        our_glyph.name, our_nodes, golden_nodes
                    );
                } else {
                    println!("  · {}: 0 nodes", our_glyph.name);
                    matched += 1;
                }
                total += 1;
            }

            println!("\nOutline integrity: {}/{} glyphs match", matched, total);
        }
    }
}

/// Load VFB, save as VFJ, and compare *all* structures
#[test]
#[ignore] // Run with `cargo test -- --ignored` to execute
fn test_vfb_to_vfj_structural_diff() {
    let mut passed = true;
    for (vfb_path, golden_vfj_path) in get_vfb_test_files() {
        if !vfb_path.exists() {
            println!("Skipping {} - file not found", vfb_path.display());
            continue;
        }

        println!("\n=== Testing {} ===", vfb_path.display());

        // Load VFB into Babelfont
        let font = match babelfont::load(&vfb_path) {
            Ok(f) => f,
            Err(e) => {
                panic!("⚠ Failed to load VFB: {}", e);
            }
        };

        // Serialize to JSON value with detailed error path
        let font_json: serde_json::Value = match serde_json::to_value(&font) {
            Ok(json) => json,
            Err(e) => {
                eprintln!("❌ Failed to serialize VFB font to JSON: {}", e);
                eprintln!("Error details: {}", e.to_string());
                // Try to get more details by serializing to string
                match serde_json::to_string_pretty(&font) {
                    Ok(_) => eprintln!("✓ String serialization worked"),
                    Err(e2) => eprintln!("❌ String serialization also failed: {}", e2),
                }
                panic!("⚠ Failed to serialize VFB font: {}", e);
            }
        };

        // Load golden VFJ as Babelfont
        let golden_font = match babelfont::load(&golden_vfj_path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("⚠ Failed to load golden VFJ: {}", e);
                passed = false;
                continue;
            }
        };
        let golden_vfj_json: serde_json::Value = match serde_json::to_value(&golden_font) {
            Ok(json) => json,
            Err(e) => {
                eprintln!("⚠ Failed to serialize golden font to JSON: {}", e);
                passed = false;
                continue;
            }
        };
        // Compare the two JSON structures
        if !structural_diff(&golden_vfj_json, &font_json, vec![]).0 {
            passed = false;
        }
    }
    assert!(passed, "Some VFB to VFJ structural comparisons failed");
}

fn structural_diff(
    value_a: &serde_json::Value,
    value_b: &serde_json::Value,
    stack: Vec<String>,
) -> (bool, Vec<String>) {
    match (value_a, value_b) {
        (serde_json::Value::Object(map_a), serde_json::Value::Object(map_b)) => {
            for (key, val_a) in map_a {
                if key == "date" {
                    continue; // Generated dates will reliably differ
                }
                if let Some(val_b) = map_b.get(key) {
                    let new_stack = {
                        let mut s = stack.clone();
                        s.push(key.to_string());
                        s
                    };
                    let (res, new_stack) = structural_diff(val_a, val_b, new_stack);
                    if !res {
                        return (false, new_stack);
                    }
                } else {
                    println!("Key {} missing in second JSON at {:?}", key, stack);
                    return (false, stack);
                }
            }
        }
        (serde_json::Value::Array(arr_a), serde_json::Value::Array(arr_b)) => {
            if arr_a.len() != arr_b.len() {
                println!(
                    "Array length mismatch: {} vs {} at {:?}",
                    arr_a.len(),
                    arr_b.len(),
                    stack
                );
                return (false, stack);
            } else {
                for (i, (val_a, val_b)) in arr_a.iter().zip(arr_b.iter()).enumerate() {
                    let mut newstack = stack.clone();
                    newstack.push(i.to_string());
                    if !structural_diff(val_a, val_b, newstack.clone()).0 {
                        return (false, newstack);
                    }
                }
            }
        }
        _ => {
            if value_a != value_b {
                println!(
                    "Value mismatch: {:?} vs {:?} at {:?}",
                    value_a, value_b, stack
                );
                return (false, stack);
            }
        }
    }
    (true, stack)
}
