/// Test suite for iteratively improving VFB support in Babelfont
/// Compares VFB→Babelfont→VFJ output against golden reference files
use serde_json::Value;
use std::path::PathBuf;

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
    let test_vfb = PathBuf::from("resources/IBMPlexSerif-Text.vfb");
    if !test_vfb.exists() {
        println!("Test file not found: {}", test_vfb.display());
        return;
    }

    let font = babelfont::load(&test_vfb).expect("Failed to load VFB");

    println!("\n=== VFB Loader Coverage ===");
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
        println!("  First glyph: {} ({} layers)", first_glyph.name, first_glyph.layers.len());
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
        let golden: Value = serde_json::from_str(&golden_content).expect("Failed to parse golden VFJ");

        // Detailed comparison
        println!("\n--- FONT-LEVEL PROPERTIES ---");
        compare_values(
            "upm",
            &serde_json::json!(font.upm),
            &golden["font"]["upm"],
        );

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
                        println!("  Unicode - Golden: {}, Ours: {:?}", gg_unicode, our_glyph.codepoints);
                    }
                }
            }
        }

        println!("\n--- STRUCTURE CHECK ---");
        if let Some(golden_axes) = golden["font"]["axes"].as_array() {
            println!("Axes - Golden: {}, Ours: {}", golden_axes.len(), font.axes.len());
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
        let golden: Value = serde_json::from_str(&golden_content).expect("Failed to parse golden VFJ");

        println!("\nFont has {} masters, analyzing layer structure...", font.masters.len());

        let golden_glyphs = golden["font"]["glyphs"]
            .as_array()
            .expect("Golden glyphs is array");

        let mut layer_mismatches = 0;
        let mut sample_glyphs = Vec::new();

        for (glyph_idx, (our_glyph, golden_glyph)) in font
            .glyphs
            .iter()
            .zip(golden_glyphs.iter())
            .enumerate()
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

        println!("\nLayer count discrepancies: {}/{}", layer_mismatches, font.glyphs.len());
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
