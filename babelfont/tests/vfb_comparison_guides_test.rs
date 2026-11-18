#[cfg(test)]
mod test_guides {
    #[test]
    fn test_negative_guide_count_does_not_lose_glyph_guides() {
        use babelfont::{load, LayerType};

        let font = load("resources/Arvo_ita_MM_54_b_sorce.vfb").expect("Failed to load");
        let glyph = font
            .glyphs
            .iter()
            .find(|g| g.name == "S")
            .expect("Glyph S not found");

        let master_ids: Vec<String> = font.masters.iter().map(|m| m.id.clone()).collect();
        let x_positions = |master_id: &str| -> Vec<f32> {
            let layer = glyph
                .layers
                .iter()
                .find(|l| l.master == LayerType::DefaultForMaster(master_id.to_string()))
                .expect("layer for master");
            layer
                .guides
                .iter()
                .map(|g| g.pos.x)
                .filter(|x| *x > 0.0)
                .collect()
        };

        // FontLab wrote a negative horizontal guide count for `S`, but the
        // three vertical guides are still valid and must be loaded.
        assert_eq!(x_positions(&master_ids[0]), vec![567.0, 752.0, 377.0]);
        assert_eq!(x_positions(&master_ids[1]), vec![485.0, 669.0, 294.0]);
    }

    #[test]
    #[ignore]
    fn test_guides_loaded() {
        use babelfont::load;

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
            println!("\n{}", name);
            let font = load(path).expect("Failed to load");

            let mut total_guides = 0;
            let mut masters_with_guides = 0;

            for master in font.masters.iter() {
                let master_guides = master.guides.len();
                if master_guides > 0 {
                    masters_with_guides += 1;
                    let name = master.name.get_default().map(|s| s.as_str()).unwrap_or("?");
                    println!("  Master '{}': {} guides", name, master_guides);
                    for guide in &master.guides {
                        println!(
                            "    - x={}, y={}, angle={}",
                            guide.pos.x, guide.pos.y, guide.pos.angle
                        );
                    }
                }
                total_guides += master_guides;
            }

            println!("  Masters with guides: {}", masters_with_guides);
            println!("  Total guides: {}", total_guides);

            if total_guides == 0 {
                println!("  ⚠ No guides found!");
            } else {
                println!("  ✓ Guides loaded successfully");
            }
        }
    }
}
