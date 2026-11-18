#[cfg(test)]
mod test_layer_names {
    #[test]
    #[ignore]
    fn test_layer_names_and_masks() {
        use babelfont::load;

        let test_files = vec![
            (
                "IBMPlexSerif-Text.vfb",
                "resources/IBMPlexSerif-Text.vfb",
                2,
                true,
            ), // 2 layers (mask + master), has mask
            (
                "Arvo_reg_MM_12b_2TT_sorce.vfb",
                "resources/Arvo_reg_MM_12b_2TT_sorce.vfb",
                3,
                true,
            ), // 3 layers (img + 2 masters), has mask
            (
                "Arvo_ita_MM_54_b_sorce.vfb",
                "resources/Arvo_ita_MM_54_b_sorce.vfb",
                2,
                true,
            ), // Check actual value
        ];

        for (name, path, expected_layers, _has_mask) in test_files {
            println!("\n{}", name);
            let font = load(path).expect("Failed to load");

            let first_glyph = &font.glyphs[0];
            let layer_count = first_glyph.layers.len();

            println!("  Glyph '{}': {} layers", first_glyph.name, layer_count);

            let mut has_mask = false;
            let mut has_img = false;
            let mut has_named_master = false;

            for (i, layer) in first_glyph.layers.iter().enumerate() {
                let layer_name = layer.name.as_deref().unwrap_or("(unnamed)");
                println!("    Layer {}: '{}'", i, layer_name);

                if layer_name == "mask" {
                    has_mask = true;
                }
                if layer_name == "img" {
                    has_img = true;
                }
                if layer_name != "mask" && layer_name != "img" && layer_name != "(unnamed)" {
                    has_named_master = true;
                }
            }

            println!("  Summary:");
            println!("    Layer count: {}", layer_count);
            if has_mask {
                println!("    ✓ Mask layer present");
            }
            if has_img {
                println!("    ✓ Image layer present");
            }
            if has_named_master {
                println!("    ✓ Named master layers present");
            }

            if layer_count != expected_layers && expected_layers > 0 {
                println!(
                    "    ⚠ Expected {} layers but got {}",
                    expected_layers, layer_count
                );
            }
        }
    }
}
