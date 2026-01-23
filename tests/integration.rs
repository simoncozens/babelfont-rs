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
