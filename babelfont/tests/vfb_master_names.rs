#[test]
#[ignore]
fn check_master_names() {
    use vfbreader::{read_vfb, VfbEntry};
    
    let test_files = vec![
        ("IBMPlexSerif-Text.vfb", "resources/IBMPlexSerif-Text.vfb"),
        ("Arvo_reg_MM_12b_2TT_sorce.vfb", "resources/Arvo_reg_MM_12b_2TT_sorce.vfb"),
    ];

    for (name, path) in test_files {
        let vfb = read_vfb(std::path::Path::new(path)).expect("Failed to load");
        
        println!("\n{}", name);
        
        let mut master_names = vec![];
        let mut master_count = 0;
        
        for entry in &vfb.entries {
            match entry {
                VfbEntry::MasterCount(c) => master_count = *c as usize,
                VfbEntry::MasterName(s) => master_names.push(s.clone()),
                _ => {}
            }
        }
        
        println!("  MasterCount: {}", master_count);
        for (i, name) in master_names.iter().enumerate() {
            println!("  MasterName[{}]: '{}'", i, name.trim());
        }
    }
}
