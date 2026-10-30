use buildcraft3d::io::threemf::{self, format_transform, parse_model, parse_transform};
use buildcraft3d::io::ModelObject;
use buildcraft3d::primitives;
use glam::{Mat4, Vec3};
use std::io::Read;

/// A one-triangle object (id 1) at (0,0,0), (1,0,0), (0,1,0) plus extra XML.
fn model(unit_attr: &str, extra_resources: &str, build: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<model {unit_attr} xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">
<resources>
<object id="1" type="model"><mesh>
<vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/></vertices>
<triangles><triangle v1="0" v2="1" v3="2"/></triangles>
</mesh></object>
{extra_resources}
</resources>
<build>{build}</build>
</model>"#
    )
}

fn world(o: &ModelObject, i: usize) -> Vec3 {
    o.transform.transform_point3(o.mesh.positions[i])
}

fn close(a: Vec3, b: Vec3) -> bool {
    (a - b).length() < 1e-4
}

#[test]
fn inch_unit_scales_coordinates_by_25_4() {
    let o = parse_model(&model(r#"unit="inch""#, "", r#"<item objectid="1"/>"#), "n").unwrap();
    assert!(close(o[0].mesh.positions[1], Vec3::new(25.4, 0.0, 0.0)));
}

#[test]
fn micron_unit_scales_coordinates_by_0_001() {
    let o = parse_model(
        &model(r#"unit="micron""#, "", r#"<item objectid="1"/>"#),
        "n",
    )
    .unwrap();
    assert!(close(o[0].mesh.positions[2], Vec3::new(0.0, 0.001, 0.0)));
}

#[test]
fn default_and_millimeter_units_do_not_scale() {
    for unit in ["", r#"unit="millimeter""#] {
        let o = parse_model(&model(unit, "", r#"<item objectid="1"/>"#), "n").unwrap();
        assert!(close(o[0].mesh.positions[1], Vec3::X), "{unit}");
    }
}

#[test]
fn unit_scale_also_applies_to_item_translation_but_not_to_its_rotation() {
    let item = r#"<item objectid="1" transform="0 1 0 -1 0 0 0 0 1 2 0 0"/>"#;
    let o = parse_model(&model(r#"unit="centimeter""#, "", item), "n").unwrap();
    // Vertex (1,0,0) cm = 10 mm, rotated 90 degrees about Z to +Y, plus 2 cm along X.
    assert!(
        close(world(&o[0], 1), Vec3::new(20.0, 10.0, 0.0)),
        "{:?}",
        world(&o[0], 1)
    );
}

#[test]
fn nested_component_transforms_are_composed_parent_after_child() {
    // Object 3 holds object 2 shifted +10 X; object 2 holds object 1 rotated 90 degrees about Z.
    let res = r#"
<object id="2"><components><component objectid="1" transform="0 1 0 -1 0 0 0 0 1 0 0 0"/></components></object>
<object id="3"><components><component objectid="2" transform="1 0 0 0 1 0 0 0 1 10 0 0"/></components></object>"#;
    let o = parse_model(
        &model(
            "",
            res,
            r#"<item objectid="3" transform="1 0 0 0 1 0 0 0 1 0 0 7"/>"#,
        ),
        "n",
    )
    .unwrap();
    assert_eq!(o.len(), 1);
    // v=(1,0,0): rotate -> (0,1,0); shift -> (10,1,0); build item -> (10,1,7).
    assert!(
        close(world(&o[0], 1), Vec3::new(10.0, 1.0, 7.0)),
        "{:?}",
        world(&o[0], 1)
    );
    assert!(
        close(world(&o[0], 2), Vec3::new(9.0, 0.0, 7.0)),
        "{:?}",
        world(&o[0], 2)
    );
}

#[test]
fn one_object_used_by_two_components_gives_two_placed_objects() {
    let res = r#"<object id="2"><components>
<component objectid="1"/>
<component objectid="1" transform="1 0 0 0 1 0 0 0 1 5 0 0"/>
</components></object>"#;
    let o = parse_model(&model("", res, r#"<item objectid="2"/>"#), "n").unwrap();
    assert_eq!(o.len(), 2);
    assert!(close(world(&o[1], 0), Vec3::new(5.0, 0.0, 0.0)));
}

#[test]
fn object_level_colour_comes_from_basematerials_pid_and_pindex() {
    let xml = r##"<model xmlns="x"><resources>
<basematerials id="1"><base name="a" displaycolor="#112233FF"/><base name="b" displaycolor="#AABBCC80"/></basematerials>
<object id="2" pid="1" pindex="1"><mesh><vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/></vertices>
<triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object>
</resources><build><item objectid="2"/></build></model>"##;
    assert_eq!(
        parse_model(xml, "n").unwrap()[0].color,
        [0xAA, 0xBB, 0xCC, 0x80]
    );
}

#[test]
fn six_digit_colours_are_opaque() {
    let xml = r##"<model xmlns="x"><resources>
<basematerials id="1"><base name="a" displaycolor="#112233"/></basematerials>
<object id="2" pid="1" pindex="0"><mesh><vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/></vertices>
<triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh></object>
</resources><build><item objectid="2"/></build></model>"##;
    assert_eq!(
        parse_model(xml, "n").unwrap()[0].color,
        [0x11, 0x22, 0x33, 0xFF]
    );
}

#[test]
fn per_triangle_colour_without_object_colour_uses_the_most_used_one() {
    // The spec leaves a single-colour object model open; the reader picks the majority colour.
    let xml = r##"<model xmlns="x"><resources>
<basematerials id="1"><base name="a" displaycolor="#FF0000FF"/><base name="b" displaycolor="#00FF00FF"/></basematerials>
<object id="2"><mesh><vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/><vertex x="1" y="1" z="0"/></vertices>
<triangles>
<triangle v1="0" v2="1" v3="2" pid="1" p1="1"/>
<triangle v1="1" v2="3" v3="2" pid="1" p1="1"/>
<triangle v1="0" v2="1" v3="3" pid="1" p1="0"/>
</triangles></mesh></object>
</resources><build><item objectid="2"/></build></model>"##;
    assert_eq!(parse_model(xml, "n").unwrap()[0].color, [0, 255, 0, 255]);
}

#[test]
fn object_without_colour_gets_the_default_colour_and_the_default_name() {
    let o = parse_model(&model("", "", r#"<item objectid="1"/>"#), "fallback").unwrap();
    assert_eq!(o[0].color, buildcraft3d::scene::DEFAULT_COLOR);
    assert_eq!(o[0].name, "fallback");
}

#[test]
fn build_item_pointing_to_a_missing_object_is_an_error() {
    assert!(parse_model(&model("", "", r#"<item objectid="99"/>"#), "n").is_err());
}

#[test]
fn malformed_models_are_errors() {
    let item = r#"<item objectid="1"/>"#;
    // Not XML at all.
    assert!(parse_model("<model><resources></model>", "n").is_err());
    // Triangle without v3.
    let bad_tri = model("", "", item).replace(r#" v3="2""#, "");
    assert!(parse_model(&bad_tri, "n").is_err());
    // Triangle index beyond the vertices.
    let bad_idx = model("", "", item).replace(r#"v3="2""#, r#"v3="9""#);
    assert!(parse_model(&bad_idx, "n").is_err());
    // Bad item transform.
    let bad_t = model("", "", r#"<item objectid="1" transform="1 2 3"/>"#);
    assert!(parse_model(&bad_t, "n").is_err());
}

#[test]
fn cyclic_components_are_an_error_not_a_stack_overflow() {
    let res = r#"<object id="2"><components><component objectid="3"/></components></object>
<object id="3"><components><component objectid="2"/></components></object>"#;
    assert!(parse_model(&model("", res, r#"<item objectid="2"/>"#), "n").is_err());
}

#[test]
fn parse_transform_rejects_bad_input() {
    for bad in [
        "",
        "   ",
        "1 2 3",
        "1 0 0 0 1 0 0 0 1 0 0",
        "1 0 0 0 1 0 0 0 1 0 0 0 0",
        "1 0 0 0 1 0 0 0 1 0 0 x",
    ] {
        assert!(parse_transform(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn parse_transform_reads_twelve_values_column_by_column() {
    assert_eq!(
        parse_transform("1 0 0 0 1 0 0 0 1 0 0 0").unwrap(),
        Mat4::IDENTITY
    );
    let m = parse_transform("1 0 0 0 1 0 0 0 1 5 6 7").unwrap();
    assert!(close(
        m.transform_point3(Vec3::ZERO),
        Vec3::new(5.0, 6.0, 7.0)
    ));
    // Third triple is the Z column.
    let m = parse_transform("1 0 0 0 1 0 0 0 3 0 0 0").unwrap();
    assert!(close(m.transform_point3(Vec3::Z), Vec3::new(0.0, 0.0, 3.0)));
}

#[test]
fn format_transform_then_parse_transform_round_trips() {
    let m = Mat4::from_scale_rotation_translation(
        Vec3::new(1.5, 2.0, 0.25),
        glam::Quat::from_euler(glam::EulerRot::ZYX, 0.3, -0.2, 1.1),
        Vec3::new(10.5, -3.25, 0.125),
    );
    let back = parse_transform(&format_transform(&m)).unwrap();
    assert!(back.abs_diff_eq(m, 1e-6));
}

fn model_xml_in_zip(path: &std::path::Path) -> String {
    let mut z = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut s = String::new();
    z.by_name("3D/3dmodel.model")
        .unwrap()
        .read_to_string(&mut s)
        .unwrap();
    s
}

fn sample() -> Vec<ModelObject> {
    vec![ModelObject {
        name: "Cube & <co>".into(),
        mesh: primitives::cube(10.0),
        transform: Mat4::from_translation(Vec3::new(3.0, 4.0, 5.0)),
        color: [0xFE, 0xF1, 0x00, 0xFF],
    }]
}

#[test]
fn title_metadata_is_written_only_when_a_title_is_given() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.3mf"), dir.path().join("b.3mf"));
    threemf::write(&a, &sample(), Some("My title")).unwrap();
    threemf::write(&b, &sample(), None).unwrap();
    let with = model_xml_in_zip(&a);
    let without = model_xml_in_zip(&b);
    assert!(
        with.contains(r#"<metadata name="Title">My title</metadata>"#),
        "{with}"
    );
    assert!(!without.contains("Title") && !without.contains("<metadata"));
}

#[test]
fn write_escapes_the_title_and_object_names_so_the_file_reads_back() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("e.3mf");
    threemf::write(&p, &sample(), Some("a < b & \"c\"")).unwrap();
    let back = threemf::read(&p).unwrap();
    assert_eq!(back[0].name, "Cube & <co>");
}

#[test]
fn write_then_read_keeps_name_colour_transform_and_mesh() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("r.3mf");
    let objs = sample();
    threemf::write(&p, &objs, None).unwrap();
    let back = threemf::read(&p).unwrap();
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].color, objs[0].color);
    assert!(back[0].transform.abs_diff_eq(objs[0].transform, 1e-6));
    assert_eq!(back[0].mesh.triangles, objs[0].mesh.triangles);
    assert_eq!(back[0].mesh.positions, objs[0].mesh.positions);
}

#[test]
fn write_is_atomic_and_leaves_no_temporary_file() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("a.3mf");
    threemf::write(&p, &sample(), None).unwrap();
    threemf::write(&p, &sample(), Some("again")).unwrap();
    let names: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["a.3mf"]);
}

#[test]
fn failed_write_keeps_the_previous_file_intact() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("keep.3mf");
    threemf::write(&p, &sample(), Some("old")).unwrap();
    let before = std::fs::read(&p).unwrap();
    // A target in a directory that does not exist cannot be created.
    let missing = dir.path().join("no_such_dir").join("x.3mf");
    assert!(threemf::write(&missing, &sample(), None).is_err());
    assert_eq!(std::fs::read(&p).unwrap(), before);
    assert!(!missing.exists());
}

#[test]
fn non_zip_file_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("bad.3mf");
    std::fs::write(&p, b"not a zip").unwrap();
    assert!(threemf::read(&p).is_err());
}

#[test]
fn write_to_memory_produces_a_readable_package() {
    let mut buf = std::io::Cursor::new(Vec::new());
    threemf::write_to(&mut buf, &sample(), Some("t")).unwrap();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(buf.into_inner())).unwrap();
    for name in ["[Content_Types].xml", "_rels/.rels", "3D/3dmodel.model"] {
        assert!(z.by_name(name).is_ok(), "{name}");
    }
}

#[test]
fn painted_triangles_round_trip_through_3mf() {
    let mut mesh = primitives::cube(10.0);
    mesh.colors = vec![[0; 4]; mesh.triangles.len()];
    for f in mesh.face_region(0, 10.0) {
        mesh.colors[f] = [255, 0, 0, 255];
    }
    let painted: Vec<usize> = (0..mesh.triangles.len())
        .filter(|i| mesh.face_color(*i).is_some())
        .collect();
    assert_eq!(painted.len(), 2);
    let obj = ModelObject {
        name: "Cube".into(),
        mesh,
        color: [0x00, 0xA0, 0xE8, 0xFF],
        transform: Mat4::IDENTITY,
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("painted.3mf");
    threemf::write(&path, std::slice::from_ref(&obj), None).unwrap();
    let back = threemf::read(&path).unwrap();
    assert_eq!(back.len(), 1);
    let m = &back[0].mesh;
    for i in 0..m.triangles.len() {
        let want = painted.contains(&i).then_some([255, 0, 0, 255]);
        // Unpainted faces come back with the object's colour, painted ones with red.
        let got = m.face_color(i).filter(|c| *c != obj.color);
        assert_eq!(got, want, "triangle {i}");
    }
    assert_eq!(back[0].color, obj.color);
}
