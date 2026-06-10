use babelfont::{load, BabelfontError, Shape, Tag};
use fontdrasil::coords::UserCoord;
use kurbo::Affine;
use pretty_assertions::assert_eq;

#[test]
fn test_load_babelfont() -> Result<(), BabelfontError> {
    let path = "resources/RadioCanadaDisplay.babelfont";
    let font = load(path)?;

    assert_eq!(font.upm, 1000);
    assert_eq!(font.version, (1, 1));
    assert_eq!(font.axes.len(), 1);
    let wght_axis = font
        .axes
        .iter()
        .find(|ax| ax.tag == Tag::new(b"wght"))
        .unwrap();
    assert_eq!(wght_axis.name.get_default().unwrap(), "Weight");
    assert_eq!(wght_axis.min, Some(UserCoord::new(400.0)));

    assert_eq!(font.masters.len(), 2);
    let first_master = &font.masters[0];
    assert_eq!(first_master.name.get_default().unwrap(), "Regular");
    let wght_loc = first_master.location.get(Tag::new(b"wght")).unwrap();
    assert_eq!(
        wght_axis.designspace_to_userspace(wght_loc).unwrap(),
        UserCoord::new(400.0)
    );

    let second_master = &font.masters[1];
    let wght_loc2 = second_master.location.get(Tag::new(b"wght")).unwrap();
    assert_eq!(
        wght_axis.designspace_to_userspace(wght_loc2).unwrap(),
        UserCoord::new(700.0)
    );

    assert_eq!(font.instances.len(), 4);
    let first_instance = &font.instances[0];
    assert_eq!(first_instance.name.get_default().unwrap(), "Regular");

    assert_eq!(font.glyphs.len(), 477);

    let aacute = font.glyphs.get("Aacute").unwrap();
    assert_eq!(aacute.layers.len(), 4);
    let first_layer = &aacute.layers[0];
    assert_eq!(first_layer.shapes.len(), 2);
    if let Shape::Component(c) = &first_layer.shapes[1] {
        assert_eq!(c.transform.as_affine(), Affine::translate((87.0, 0.0)));
    } else {
        panic!("Should be a component");
    }

    assert_eq!(
        font.names.family_name.get_default().unwrap(),
        "Radio Canada Display"
    );

    assert!(!font.first_kern_groups.is_empty());
    assert!(!font.second_kern_groups.is_empty());
    assert!(!font.features.features.is_empty());

    Ok(())
}

#[test]
fn test_convert_to_ttf_radiocanada() {
    let path = "resources/RadioCanadaDisplay.babelfont";
    let font = load(path).expect("Failed to load babelfont");

    use babelfont::convertors::fontir::CompilationOptions;
    use write_fonts::read::TableProvider;
    let bytes = babelfont::convertors::fontir::BabelfontIrSource::compile(
        font.clone(),
        CompilationOptions::default(),
    )
    .expect("Failed to compile to TTF");
    // Check we have a fvar table
    use write_fonts::read::FontRef;
    let font_ref = FontRef::new(&bytes).expect("Failed to read font bytes");
    assert!(font_ref.fvar().is_ok());
    // Check we have no VARC table (even though we asked for one)
    assert!(font_ref.varc().is_err());
}

#[test]
fn test_convert_to_ttf_grantha() {
    let path = "resources/NotoSansGrantha-SmartComponent.glyphs";
    let font = load(path).expect("Failed to load babelfont");

    use babelfont::convertors::fontir::CompilationOptions;
    use write_fonts::read::TableProvider;
    let bytes = babelfont::convertors::fontir::BabelfontIrSource::compile(
        font.clone(),
        CompilationOptions::default(),
    )
    .expect("Failed to compile to TTF");
    // Check we have a fvar table
    use write_fonts::read::FontRef;
    let font_ref = FontRef::new(&bytes).expect("Failed to read font bytes");
    assert!(font_ref.fvar().is_ok());
    // Check we have a VARC table
    assert!(font_ref.varc().is_ok());

    // Now do it again with varc turned off
    // XXX
}

#[test]
fn test_convert_to_ttf_static() {
    let path = "resources/NotoSans-LightItalic.ufo";
    let font = load(path).expect("Failed to load babelfont");

    use babelfont::convertors::fontir::CompilationOptions;
    use write_fonts::read::TableProvider;
    let bytes = babelfont::convertors::fontir::BabelfontIrSource::compile(
        font.clone(),
        CompilationOptions::default(),
    )
    .expect("Failed to compile to TTF");
    // Check we have no fvar table
    use write_fonts::read::FontRef;
    let font_ref = FontRef::new(&bytes).expect("Failed to read font bytes");
    assert!(font_ref.fvar().is_err());
}

#[test]
fn test_convert_to_ttf_static2() {
    let path = "resources/NotoSansLimbu.glyphs";
    let font = load(path).expect("Failed to load babelfont");

    use babelfont::convertors::fontir::CompilationOptions;
    use write_fonts::read::TableProvider;
    let bytes = babelfont::convertors::fontir::BabelfontIrSource::compile(
        font.clone(),
        CompilationOptions::default(),
    )
    .expect("Failed to compile to TTF");
    // Check we have no fvar table
    use write_fonts::read::FontRef;
    let font_ref = FontRef::new(&bytes).expect("Failed to read font bytes");
    assert!(font_ref.fvar().is_err());
}

#[test]
fn test_ufo_export_unifies_glyphs3_rtl_kerning() {
    let font = load("resources/G3RTLKerning.glyphs").expect("Failed to load RTL sample");

    let ufo = babelfont::convertors::ufo::as_norad(&font, 0)
        .expect("Failed to export Glyphs RTL sample to UFO");

    let reh_side1 = norad::Name::new("public.kern1.reh").unwrap();
    let alef_side2 = norad::Name::new("public.kern2.alef").unwrap();
    let reh_side2 = norad::Name::new("public.kern2.reh").unwrap();
    let a_side1 = norad::Name::new("public.kern1.A").unwrap();
    let t_side1 = norad::Name::new("public.kern1.T").unwrap();
    let a_side2 = norad::Name::new("public.kern2.A").unwrap();
    let t_side2 = norad::Name::new("public.kern2.T").unwrap();

    // RTL kerning (from glyphsLib convention: RTL pairs merged into LTR)
    assert_eq!(
        ufo.kerning
            .get(&reh_side1)
            .and_then(|pairs| pairs.get(&alef_side2))
            .copied(),
        Some(-90.0)
    );
    assert_eq!(
        ufo.kerning
            .get(&reh_side1)
            .and_then(|pairs| pairs.get(&reh_side2))
            .copied(),
        Some(-40.0)
    );

    // LTR kerning (original pairs preserved)
    assert_eq!(
        ufo.kerning
            .get(&a_side1)
            .and_then(|pairs| pairs.get(&t_side2))
            .copied(),
        Some(-80.0)
    );
    assert_eq!(
        ufo.kerning
            .get(&t_side1)
            .and_then(|pairs| pairs.get(&a_side2))
            .copied(),
        Some(-80.0)
    );
}
