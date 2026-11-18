#[cfg(test)]
mod test_anchors {
    #[test]
    #[ignore]
    fn test_anchors_loaded() {
        use babelfont::load;

        let test_files = vec![
            ("IBMPlexSerif-Text.vfb", "resources/IBMPlexSerif-Text.vfb"),
            (
                "Arvo_reg_MM_12b_2TT_sorce.vfb",
                "resources/Arvo_reg_MM_12b_2TT_sorce.vfb",
            ),
        ];

        for (name, path) in test_files {
            println!("\n{}", name);
            let font = load(path).expect("Failed to load");

            let mut total_anchors = 0;
            let mut glyphs_with_anchors = 0;

            for glyph in font.glyphs.iter() {
                let mut glyph_anchors = 0;
                for layer in &glyph.layers {
                    glyph_anchors += layer.anchors.len();
                    total_anchors += layer.anchors.len();
                }
                if glyph_anchors > 0 {
                    glyphs_with_anchors += 1;
                }
            }

            println!("  Glyphs with anchors: {}", glyphs_with_anchors);
            println!("  Total anchors: {}", total_anchors);

            if glyphs_with_anchors == 0 {
                println!("  ⚠ No anchors found!");
            } else {
                println!("  ✓ Anchors loaded successfully");
            }
        }
    }
}
