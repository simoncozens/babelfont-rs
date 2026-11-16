#[cfg(test)]
mod test_guides {
    #[test]
    #[ignore]
    fn test_guides_loaded() {
        use babelfont::load;

        let test_files = vec![
            ("IBMPlexSerif-Text.vfb", "resources/IBMPlexSerif-Text.vfb"),
            ("Arvo_reg_MM_12b_2TT_sorce.vfb", "resources/Arvo_reg_MM_12b_2TT_sorce.vfb"),
            ("Arvo_ita_MM_54_b_sorce.vfb", "resources/Arvo_ita_MM_54_b_sorce.vfb"),
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
                        println!("    - x={}, y={}, angle={}", guide.pos.x, guide.pos.y, guide.pos.angle);
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
