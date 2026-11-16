#[test]
#[ignore]
fn check_layer_entries() {
    use vfbreader::{read_vfb, VfbEntry};
    
    let path = "resources/IBMPlexSerif-Text.vfb";
    let vfb = read_vfb(std::path::Path::new(path)).expect("Failed to load");
    
    let mut glyph_count = 0;
    let mut layer_entries = vec![];
    
    for entry in &vfb.entries {
        match entry {
            VfbEntry::Glyph(_) => {
                glyph_count += 1;
                if glyph_count <= 5 {
                    println!("Glyph[{}]", glyph_count);
                }
            }
            VfbEntry::Mask(_) => {
                layer_entries.push((glyph_count, "Mask"));
                if glyph_count <= 5 {
                    println!("  Mask");
                }
            }
            VfbEntry::Image(img) => {
                layer_entries.push((glyph_count, "Image"));
                if glyph_count <= 5 {
                    println!("  Image");
                }
            }
            _ => {}
        }
    }
    
    println!("\nSummary of layer entries:");
    println!("  Mask: {} entries", layer_entries.iter().filter(|(_, t)| *t == "Mask").count());
    println!("  Image: {} entries", layer_entries.iter().filter(|(_, t)| *t == "Image").count());
}
