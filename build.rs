use roxmltree::ParsingOptions;
use serde::Deserialize;

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct GlyphInfo {
    // unicode: Option<u32>,
    // unicode_legacy: Option<String>,
    name: String,
    category: String,
    // sub_category: Option<String>,
    // case: Option<String>,
    // direction: Option<String>,
    // script: Option<String>,
    production: Option<String>,
    // alt_names: Vec<String>,
}

fn main() {
    let doc = roxmltree::Document::parse_with_options(
        include_str!("resources/GlyphData.xml"),
        ParsingOptions {
            allow_dtd: true,
            nodes_limit: u32::MAX,
            entity_resolver: None,
        },
    )
    .expect("Failed to parse XML");
    let mut include = "[\n".to_string();
    for node in doc.descendants() {
        let Some(name) = node.attribute("name") else {
            continue;
        };
        include.push_str(&format!("{{\"name\": \"{}\", ", name));
        // if let Some(unicode) = node.attribute("unicode") {
        //     let codepoint = u32::from_str_radix(unicode, 16)
        //         .expect(format!("Invalid unicode value: {}", unicode).as_str());
        //     include.push_str(&format!("unicode: {},", codepoint));
        // }
        if let Some(category) = node.attribute("category") {
            include.push_str(&format!("\"category\": \"{}\",", category));
        }
        if let Some(production_name) = node.attribute("production") {
            include.push_str(&format!("\"production\": \"{}\",", production_name));
        };
        include.pop(); // Remove trailing comma
        include.push_str("},\n");
    }
    include.pop(); // Remove trailing newline
    include.pop(); // Remove trailing comma
    include.push_str("]\n");

    let out_dir = std::env::var_os("OUT_DIR").unwrap();
    let dest_path = std::path::Path::new(&out_dir).join("glyphsdata.json");
    std::fs::write(dest_path, &include).expect("Failed to write glyphs data");

    // Check we can load it now, panic at build rather than at runtime...
    let _glyphs_data: Vec<GlyphInfo> =
        serde_json::from_str(&include).expect("Failed to parse generated glyphs data");

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=resources/GlyphData.xml");
}
