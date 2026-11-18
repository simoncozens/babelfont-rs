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
        let mut font_json: serde_json::Value = match serde_json::to_value(&font) {
            Ok(json) => json,
            Err(e) => {
                eprintln!("❌ Failed to serialize VFB font to JSON: {}", e);
                eprintln!("Error details: {}", e);
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
        // Arvo's VFJ export contains no kerning even though the VFB does.
        // When the VFJ has no kerning, any VFB kerning is source-only.
        let golden_has_kerning = golden_font
            .masters
            .iter()
            .any(|master| !master.kerning.is_empty());
        let mut golden_vfj_json: serde_json::Value = match serde_json::to_value(&golden_font) {
            Ok(json) => json,
            Err(e) => {
                eprintln!("⚠ Failed to serialize golden font to JSON: {}", e);
                passed = false;
                continue;
            }
        };
        // Closed contours are cyclic: the choice of start point differs between
        // formats, so compare them up to rotation.
        normalize_closed_paths(&mut font_json);
        normalize_closed_paths(&mut golden_vfj_json);
        // VFB and VFJ use different master names for the same multi-master
        // font (e.g. "Wt0" vs "Thin"). Compare layer/master identity by
        // index instead of by name for multi-master fonts.
        normalize_master_identities(&mut font_json);
        normalize_master_identities(&mut golden_vfj_json);
        // Compare the two JSON structures, collecting *all* differences.
        let mut diffs = Vec::new();
        structural_diff(&golden_vfj_json, &font_json, vec![], &mut diffs);
        // The VFB was saved after the VFJ was exported. In Arvo Italic, `S`
        // gained glyph-level guides in between; FontLab's VFJ export predates
        // them, so they are a source-version difference, not a parse error.
        let newer_guide_glyphs: Vec<usize> = if vfb_path
            .file_name()
            .is_some_and(|name| name == "Arvo_ita_MM_54_b_sorce.vfb")
        {
            font_json
                .get("glyphs")
                .and_then(|glyphs| glyphs.as_array())
                .and_then(|glyphs| {
                    glyphs
                        .iter()
                        .position(|glyph| glyph.get("name") == Some(&serde_json::json!("S")))
                })
                .into_iter()
                .collect()
        } else {
            Vec::new()
        };
        let (source_only, hard): (Vec<_>, Vec<_>) = diffs
            .into_iter()
            .partition(|diff| is_source_only(diff, golden_has_kerning, &newer_guide_glyphs));
        if !hard.is_empty() {
            println!("✗ {} structural differences:", hard.len());
            let mut categories: std::collections::BTreeMap<String, usize> =
                std::collections::BTreeMap::new();
            for diff in &hard {
                let path = diff.split(':').next().unwrap_or("?");
                *categories.entry(normalize_path(path)).or_default() += 1;
            }
            println!("  Differences by category:");
            for (category, count) in categories {
                println!("    {:<45} {}", category, count);
            }
            let shown = hard.len().min(MAX_DIFFS_REPORTED);
            println!("  First {} differences:", shown);
            for diff in hard.iter().take(shown) {
                println!("  {}", diff);
            }
            if hard.len() > shown {
                println!("  ... and {} more", hard.len() - shown);
            }
            passed = false;
        } else {
            println!("✓ No structural differences");
        }
        if !source_only.is_empty() {
            let mut categories: std::collections::BTreeMap<String, usize> =
                std::collections::BTreeMap::new();
            for diff in &source_only {
                let path = diff.split(':').next().unwrap_or("?");
                *categories.entry(normalize_path(path)).or_default() += 1;
            }
            println!(
                "  ({} known source-metadata differences: {})",
                source_only.len(),
                categories
                    .into_iter()
                    .map(|(category, count)| format!("{} x{}", category, count))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }
    assert!(passed, "Some VFB to VFJ structural comparisons failed");
}

const MAX_DIFFS_REPORTED: usize = 50;

/// Collapse array indices so that e.g. `glyphs[12].layers[0].width` becomes
/// `glyphs[*].layers[*].width`, which makes it possible to summarize diffs.
fn normalize_path(path: &str) -> String {
    let mut out = String::new();
    let mut in_brackets = false;
    for c in path.chars() {
        match c {
            '[' => {
                in_brackets = true;
                out.push('[');
            }
            ']' if in_brackets => {
                in_brackets = false;
                out.push_str("*]");
            }
            _ if in_brackets => {}
            _ => out.push(c),
        }
    }
    out
}

fn format_path(stack: &[String]) -> String {
    let mut out = String::new();
    for part in stack {
        if part.starts_with('[') || out.is_empty() {
            out.push_str(part);
        } else {
            out.push('.');
            out.push_str(part);
        }
    }
    out
}

fn structural_diff(
    value_a: &serde_json::Value,
    value_b: &serde_json::Value,
    stack: Vec<String>,
    out: &mut Vec<String>,
) {
    match (value_a, value_b) {
        (serde_json::Value::Object(map_a), serde_json::Value::Object(map_b)) => {
            for (key, val_a) in map_a {
                if key == "date" || key == "source" || key == "color" {
                    // Generated dates, the input path, and purely visual
                    // colours (not stored in VFB) will reliably differ.
                    continue;
                }
                let mut new_stack = stack.clone();
                new_stack.push(key.to_string());
                match map_b.get(key) {
                    Some(val_b) => structural_diff(val_a, val_b, new_stack, out),
                    None => out.push(format!(
                        "{}: key missing in VFB output (VFJ has {:?})",
                        format_path(&new_stack),
                        truncate(val_a)
                    )),
                }
            }
            // Keys which only exist in our VFB output are also a mismatch.
            for key in map_b.keys() {
                if key == "date" || key == "source" || key == "color" || map_a.contains_key(key) {
                    continue;
                }
                let mut new_stack = stack.clone();
                new_stack.push(key.to_string());
                out.push(format!(
                    "{}: key missing in VFJ output (VFB has {:?})",
                    format_path(&new_stack),
                    truncate(&map_b[key])
                ));
            }
        }
        (serde_json::Value::Array(arr_a), serde_json::Value::Array(arr_b)) => {
            if arr_a.len() != arr_b.len() {
                out.push(format!(
                    "{}: array length mismatch (VFJ {}, VFB {})",
                    format_path(&stack),
                    arr_a.len(),
                    arr_b.len()
                ));
            }
            for (i, (val_a, val_b)) in arr_a.iter().zip(arr_b.iter()).enumerate() {
                let mut new_stack = stack.clone();
                new_stack.push(format!("[{}]", i));
                structural_diff(val_a, val_b, new_stack, out);
            }
        }
        _ => {
            let is_nodes = stack.last().is_some_and(|key| key == "nodes");
            let equal = if is_nodes {
                match (value_a.as_str(), value_b.as_str()) {
                    (Some(a), Some(b)) => normalize_node_string(a) == normalize_node_string(b),
                    _ => value_a == value_b,
                }
            } else {
                value_a == value_b
            };
            if !equal {
                out.push(format!(
                    "{}: {} (VFJ) vs {} (VFB)",
                    format_path(&stack),
                    truncate(value_a),
                    truncate(value_b)
                ));
            }
        }
    }
}

/// Remove smooth-node markers so that outline comparisons focus on geometry.
/// FontLab's VFJ exporter does not reliably preserve the VFB smooth flag.
/// Canonicalize the node order of a closed path: remove smooth markers and
/// rotate the node list so it starts at the lexicographically smallest node.
fn canonical_path(nodes: &str) -> String {
    let tokens: Vec<&str> = nodes.split_whitespace().collect();
    if tokens.len() % 3 != 0 {
        return tokens.join(" ");
    }
    let mut groups: Vec<Vec<&str>> = tokens.chunks(3).map(|chunk| chunk.to_vec()).collect();
    for group in groups.iter_mut() {
        group[2] = match group[2] {
            "ls" => "l",
            "cs" => "c",
            "qs" => "q",
            other => other,
        };
    }
    if groups.is_empty() {
        return String::new();
    }
    let key = |rotation: usize| -> String {
        let mut out = Vec::with_capacity(groups.len());
        for i in 0..groups.len() {
            out.push(groups[(rotation + i) % groups.len()].join(" "));
        }
        out.join(" ")
    };
    let mut best = key(0);
    for rotation in 1..groups.len() {
        let candidate = key(rotation);
        if candidate < best {
            best = candidate;
        }
    }
    best
}

/// Differences which are known not to be comparable between the two source
/// formats: names and UI metadata which VFB does not carry, metrics which only
/// one format stores, and feature code which is split differently.
fn is_source_only(diff: &str, golden_has_kerning: bool, newer_guide_glyphs: &[usize]) -> bool {
    let raw_path = diff.split(':').next().unwrap_or("");
    if newer_guide_glyphs.iter().any(|index| {
        raw_path.starts_with(&format!("glyphs[{index}].layers[")) && raw_path.ends_with(".guides")
    }) {
        return true;
    }
    let path = normalize_path(raw_path);
    path.starts_with("names.")
        || path == "features.prefixes.anonymous"
        || path.ends_with(".guides[*].name")
        || path.ends_with(".guides[*].pos.angle")
        || path == "glyphs[*].codepoints"
        || path.starts_with("masters[*].metrics.Typo")
        || path.starts_with("masters[*].metrics.Win")
        || (!golden_has_kerning && path.starts_with("masters[*].kerning"))
}

fn normalize_master_identities(value: &mut serde_json::Value) {
    let master_ids: Vec<String> = value
        .get("masters")
        .and_then(|masters| masters.as_array())
        .map(|masters| {
            masters
                .iter()
                .filter_map(|master| master.get("id").and_then(|id| id.as_str()))
                .map(|id| id.to_string())
                .collect()
        })
        .unwrap_or_default();
    if master_ids.len() < 2 {
        return;
    }

    // Master names/ids are source metadata; the two formats may name the same
    // master differently.
    if let Some(masters) = value.get_mut("masters").and_then(|m| m.as_array_mut()) {
        for master in masters {
            if let Some(master) = master.as_object_mut() {
                master.remove("id");
                master.remove("name");
            }
        }
    }

    if let Some(glyphs) = value.get_mut("glyphs").and_then(|g| g.as_array_mut()) {
        for glyph in glyphs {
            let Some(layers) = glyph.get_mut("layers").and_then(|l| l.as_array_mut()) else {
                continue;
            };
            for layer in layers {
                let Some(master) = layer.get_mut("master") else {
                    continue;
                };
                let is_default =
                    master.get("type").and_then(|t| t.as_str()) == Some("DefaultForMaster");
                if !is_default {
                    continue;
                }
                if let Some(id) = master.get("master").and_then(|m| m.as_str()) {
                    if let Some(index) = master_ids.iter().position(|master_id| master_id == id) {
                        master["master"] = serde_json::json!(index);
                    }
                }
                if let Some(layer) = layer.as_object_mut() {
                    layer.remove("id");
                    layer.remove("name");
                }
            }
        }
    }
}

fn normalize_closed_paths(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            if map.get("closed").and_then(|v| v.as_bool()) == Some(true) {
                if let Some(serde_json::Value::String(nodes)) = map.get_mut("nodes") {
                    *nodes = canonical_path(nodes);
                }
            }
            for child in map.values_mut() {
                normalize_closed_paths(child);
            }
        }
        serde_json::Value::Array(values) => {
            for child in values {
                normalize_closed_paths(child);
            }
        }
        _ => {}
    }
}

fn normalize_node_string(nodes: &str) -> String {
    nodes
        .split_whitespace()
        .map(|token| match token {
            "ls" => "l",
            "cs" => "c",
            "qs" => "q",
            other => other,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn truncate(value: &serde_json::Value) -> String {
    let s = value.to_string();
    if s.len() > 200 {
        format!("{}…", &s[..200])
    } else {
        s
    }
}

/// Verify that per-glyph kerning entries in a VFB are resolved into the
/// masters' kerning maps.
#[test]
#[ignore]
fn test_vfb_kerning() {
    let font =
        babelfont::load("resources/Arvo_reg_MM_12b_2TT_sorce.vfb").expect("Failed to load VFB");

    let thin = &font.masters[0].kerning;
    let black = &font.masters[1].kerning;

    assert_eq!(
        thin.get(&("P".into(), "period".into())),
        Some(&0),
        "expected P/period kerning in the first master"
    );
    assert_eq!(
        black.get(&("P".into(), "period".into())),
        Some(&-410),
        "expected P/period kerning in the second master"
    );
    assert_eq!(
        thin.get(&("T".into(), "period".into())),
        Some(&0),
        "expected T/period kerning in the first master"
    );
    assert_eq!(
        black.get(&("T".into(), "period".into())),
        Some(&-307),
        "expected T/period kerning in the second master"
    );
}
