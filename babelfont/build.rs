use roxmltree::ParsingOptions;
use serde::Deserialize;

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct GlyphInfo {
    // unicode: Option<u32>,
    // unicode_legacy: Option<String>,
    name: String,
    category: String,
    sub_category: Option<String>,
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
    let mut entries = vec![];
    for node in doc.descendants() {
        let Some(name) = node.attribute("name") else {
            continue;
        };
        let mut entry = serde_json::Map::new();
        entry.insert("name".into(), name.into());
        for (attribute, key) in [
            ("category", "category"),
            ("subCategory", "sub_category"),
            ("production", "production"),
            ("altNames", "alt_names"),
        ] {
            if let Some(value) = node.attribute(attribute) {
                entry.insert(key.into(), value.into());
            }
        }
        entries.push(serde_json::Value::Object(entry));
    }
    let include = serde_json::to_string(&entries).expect("Failed to serialize glyphs data");

    let out_dir = std::env::var_os("OUT_DIR").unwrap();
    let dest_path = std::path::Path::new(&out_dir).join("glyphsdata.json");
    std::fs::write(dest_path, &include).expect("Failed to write glyphs data");

    // Check we can load it now, panic at build rather than at runtime...
    let _glyphs_data: Vec<GlyphInfo> =
        serde_json::from_str(&include).expect("Failed to parse generated glyphs data");

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=resources/GlyphData.xml");
}
