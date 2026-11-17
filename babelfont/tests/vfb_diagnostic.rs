#[cfg(test)]
mod vfb_diagnostic {
    use std::collections::HashMap;
    use vfbreader::read_vfb;

    #[test]
    #[ignore]
    fn diagnostic_vfb_entries() {
        let test_files = vec![
            ("IBMPlexSerif-Text.vfb", "resources/IBMPlexSerif-Text.vfb"),
            (
                "Arvo_reg_MM_12b_2TT_sorce.vfb",
                "resources/Arvo_reg_MM_12b_2TT_sorce.vfb",
            ),
            (
                "Arvo_ita_MM_54_b_sorce.vfb",
                "resources/Arvo_ita_MM_54_b_sorce.vfb",
            ),
        ];

        for (name, path) in test_files {
            let vfb = read_vfb(std::path::Path::new(path)).expect("Failed to load");

            let mut entry_counts: HashMap<String, usize> = HashMap::new();

            for entry in &vfb.entries {
                let key = format!("{:?}", entry)
                    .split('(')
                    .next()
                    .unwrap_or("Unknown")
                    .to_string();
                *entry_counts.entry(key).or_insert(0) += 1;
            }

            println!("\n{}", name);
            println!("  Total entries: {}", vfb.entries.len());

            // Print relevant entry types
            let relevant = vec![
                "Glyph",
                "GlobalGuides",
                "GlobalGuideProperties",
                "OpenTypeClass",
                "Features",
                "MMKernPair",
                "AnchorsMm",
                "AnchorsProperties",
                "OpenTypeFeatures",
            ];

            println!("  Relevant entries:");
            for entry_type in relevant {
                if let Some(count) = entry_counts.get(entry_type) {
                    println!("    {}: {}", entry_type, count);
                }
            }
        }
    }
}

#[test]
#[ignore]
fn trace_anchors_to_glyphs() {
    use vfbreader::{read_vfb, VfbEntry};

    let vfb = read_vfb(std::path::Path::new(
        "resources/Arvo_reg_MM_12b_2TT_sorce.vfb",
    ))
    .unwrap();

    let mut glyph_index = 0;
    let mut anchor_entries = vec![];

    for entry in &vfb.entries {
        match entry {
            VfbEntry::Glyph(_) => {
                println!("Glyph[{}]", glyph_index);
                glyph_index += 1;
            }
            VfbEntry::AnchorsMm(anchor_data) => {
                println!("AnchorsMm: {} per-master anchor sets", anchor_data.len());
                for (master_idx, anchors) in anchor_data.iter().enumerate() {
                    println!("  Master {}: {} anchors", master_idx, anchors.len());
                    if !anchors.is_empty() {
                        println!("    First anchor: ({}, {})", anchors[0].x, anchors[0].y);
                    }
                }
                anchor_entries.push(("AnchorsMm", glyph_index));
            }
            VfbEntry::AnchorsProperties(props) => {
                println!("AnchorsProperties: {} entries", props.len());
                anchor_entries.push(("AnchorsProperties", glyph_index));
            }
            _ => {}
        }
    }

    println!(
        "\nAnchor entries found at glyph indices: {:?}",
        anchor_entries
    );
}

#[test]
#[ignore]
fn trace_ibmplex_anchors() {
    use vfbreader::{read_vfb, VfbEntry};

    let vfb = read_vfb(std::path::Path::new("resources/IBMPlexSerif-Text.vfb")).unwrap();

    let mut glyph_count = 0;
    let mut anchor_entries_found = vec![];

    for entry in &vfb.entries {
        match entry {
            VfbEntry::Glyph(_) => {
                glyph_count += 1;
            }
            VfbEntry::AnchorsProperties(props) => {
                anchor_entries_found
                    .push((glyph_count, format!("AnchorsProperties({})", props.len())));
            }
            VfbEntry::AnchorsMm(anchor_data) => {
                let total_anchors: usize = anchor_data.iter().map(|v| v.len()).sum();
                anchor_entries_found.push((glyph_count, format!("AnchorsMm({})", total_anchors)));
            }
            _ => {}
        }
    }

    println!("IBMPlexSerif-Text.vfb anchor entries:");
    for (glyph_idx, entry_type) in anchor_entries_found.iter().take(10) {
        println!("  After glyph {}: {}", glyph_idx, entry_type);
    }
    println!("  ... (showing first 10 of {})", anchor_entries_found.len());
}
