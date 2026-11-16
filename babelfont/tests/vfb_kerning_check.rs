#[test]
#[ignore]
fn check_kerning_entries() {
    use vfbreader::{read_vfb, VfbEntry};
    use std::collections::HashMap;
    
    let test_files = vec![
        ("IBMPlexSerif-Text.vfb", "resources/IBMPlexSerif-Text.vfb"),
        ("Arvo_reg_MM_12b_2TT_sorce.vfb", "resources/Arvo_reg_MM_12b_2TT_sorce.vfb"),
        ("Arvo_ita_MM_54_b_sorce.vfb", "resources/Arvo_ita_MM_54_b_sorce.vfb"),
    ];

    for (name, path) in test_files {
        let vfb = read_vfb(std::path::Path::new(path)).expect("Failed to load");
        
        let mut kerning_pairs = 0;
        let mut kerning_classes = 0;
        
        for entry in &vfb.entries {
            match entry {
                VfbEntry::MMKernPair(_) => kerning_pairs += 1,
                VfbEntry::OpenTypeClass(s) => {
                    if s.contains("@") {
                        kerning_classes += 1;
                    }
                }
                _ => {}
            }
        }
        
        println!("\n{}", name);
        if kerning_pairs > 0 {
            println!("  MMKernPair entries: {}", kerning_pairs);
        }
        if kerning_classes > 0 {
            println!("  Kerning classes (OpenTypeClass): {}", kerning_classes);
        }
        if kerning_pairs == 0 && kerning_classes == 0 {
            println!("  (no kerning data)");
        }
    }
}
