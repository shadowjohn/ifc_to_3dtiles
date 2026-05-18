use ifc_to_3dtiles::{
    b3dm,
    convert::median_origin,
    crs,
    geometry::{self, MeshBuildOptions},
    model::StyleTable,
    step::{StepIndex, decode_ifc_string},
};

#[test]
fn step_index_handles_multiline_entities_and_wrapped_strings() {
    let ifc = "ISO-10303-21;\nDATA;\n#1=IFCBUILDINGELEMENTPROXY('abc',#2,'','0\n, DJB-M-SU-\\X2\\76E36E2C\\X0\\.dgn, Default:5317',$,#3,#4,$,$);\n#2=IFCOWNERHISTORY();\nENDSEC;";
    let index = StepIndex::parse(ifc);

    let entity = index.entity(1).expect("entity #1");

    assert_eq!(entity.type_name, "IFCBUILDINGELEMENTPROXY");
    assert_eq!(
        decode_ifc_string("'DJB-M-SU-\\X2\\76E36E2C\\X0\\'"),
        "DJB-M-SU-監測"
    );
    assert!(index.body(entity).contains("Default:5317"));
}

#[test]
fn style_table_resolves_ifc_styled_item_rgb() {
    let ifc = "\
#10=IFCCOLOURRGB($,0.25,0.5,0.75);
#11=IFCSURFACESTYLESHADING(#10);
#12=IFCSURFACESTYLE($,.BOTH.,(#11));
#13=IFCPRESENTATIONSTYLEASSIGNMENT((#12));
#14=IFCSTYLEDITEM(#99,(#13),$);
";
    let index = StepIndex::parse(ifc);
    let styles = StyleTable::from_index(&index);

    assert_eq!(styles.color_for_item(99), Some([0.25, 0.5, 0.75, 1.0]));
}

#[test]
fn b3dm_header_uses_padded_sections_and_batch_length() {
    let glb = vec![0x67, 0x6c, 0x54, 0x46, 2, 0, 0, 0, 12, 0, 0, 0];
    let batch = serde_json::json!({
        "global_id": ["a", "b"],
        "ifc_step_id": [1, 2]
    });

    let bytes = b3dm::build_b3dm(&glb, 2, &batch).expect("b3dm");

    assert_eq!(&bytes[0..4], b"b3dm");
    assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 1);
    assert_eq!(
        u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize,
        bytes.len()
    );
    assert_eq!(
        (28 + u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize) % 8,
        0
    );
}

#[test]
fn epsg_3826_origin_converts_to_central_meridian() {
    let lon_lat = crs::project_to_wgs84(3826, 250000.0, 0.0).expect("project");

    assert!((lon_lat.lon_deg - 121.0).abs() < 1e-8);
    assert!(lon_lat.lat_deg.abs() < 1e-8);
}

#[test]
fn faceted_brep_cube_generates_twelve_triangles() {
    let ifc = "\
#1=IFCCARTESIANPOINT((0.,0.,0.));
#2=IFCCARTESIANPOINT((1.,0.,0.));
#3=IFCCARTESIANPOINT((1.,1.,0.));
#4=IFCCARTESIANPOINT((0.,1.,0.));
#5=IFCCARTESIANPOINT((0.,0.,1.));
#6=IFCCARTESIANPOINT((1.,0.,1.));
#7=IFCCARTESIANPOINT((1.,1.,1.));
#8=IFCCARTESIANPOINT((0.,1.,1.));
#11=IFCPOLYLOOP((#1,#2,#3,#4));
#12=IFCPOLYLOOP((#5,#8,#7,#6));
#13=IFCPOLYLOOP((#1,#5,#6,#2));
#14=IFCPOLYLOOP((#2,#6,#7,#3));
#15=IFCPOLYLOOP((#3,#7,#8,#4));
#16=IFCPOLYLOOP((#4,#8,#5,#1));
#21=IFCFACEOUTERBOUND(#11,.T.);
#22=IFCFACEOUTERBOUND(#12,.T.);
#23=IFCFACEOUTERBOUND(#13,.T.);
#24=IFCFACEOUTERBOUND(#14,.T.);
#25=IFCFACEOUTERBOUND(#15,.T.);
#26=IFCFACEOUTERBOUND(#16,.T.);
#31=IFCFACE((#21));
#32=IFCFACE((#22));
#33=IFCFACE((#23));
#34=IFCFACE((#24));
#35=IFCFACE((#25));
#36=IFCFACE((#26));
#40=IFCCLOSEDSHELL((#31,#32,#33,#34,#35,#36));
#50=IFCFACETEDBREP(#40);
";
    let index = StepIndex::parse(ifc);
    let mesh = geometry::mesh_faceted_brep(
        &index,
        50,
        &geometry::Mat4::identity(),
        MeshBuildOptions {
            batch_id: 7,
            color: [1.0, 0.0, 0.0, 1.0],
        },
    )
    .expect("mesh");

    assert_eq!(mesh.triangle_count(), 12);
    assert!(
        mesh.positions
            .iter()
            .all(|p| p.iter().all(|v| v.is_finite()))
    );
    assert!(mesh.batch_ids.iter().all(|id| *id == 7));
}

#[test]
fn shell_based_surface_model_generates_triangles() {
    let ifc = "\
#1=IFCCARTESIANPOINT((0.,0.,0.));
#2=IFCCARTESIANPOINT((1.,0.,0.));
#3=IFCCARTESIANPOINT((1.,1.,0.));
#4=IFCCARTESIANPOINT((0.,1.,0.));
#11=IFCPOLYLOOP((#1,#2,#3,#4));
#21=IFCFACEOUTERBOUND(#11,.T.);
#31=IFCFACE((#21));
#40=IFCOPENSHELL((#31));
#50=IFCSHELLBASEDSURFACEMODEL((#40));
";
    let index = StepIndex::parse(ifc);
    let mesh = geometry::mesh_shell_based_surface_model(
        &index,
        50,
        &geometry::Mat4::identity(),
        MeshBuildOptions {
            batch_id: 3,
            color: [0.0, 1.0, 0.0, 1.0],
        },
    )
    .expect("surface model mesh");

    assert_eq!(mesh.triangle_count(), 2);
    assert!(mesh.batch_ids.iter().all(|id| *id == 3));
}

#[test]
fn projected_coordinate_detection_sees_absolute_ifc_vertices() {
    let ifc = "\
#1=IFCCARTESIANPOINT((292153.211476,2785320.469154,24.486251));
#2=IFCCARTESIANPOINT((292154.0,2785321.0,24.0));
#3=IFCCARTESIANPOINT((292155.0,2785321.0,24.0));
#4=IFCCARTESIANPOINT((0.1,0.2,0.3));
#11=IFCPOLYLOOP((#1,#2,#3));
#12=IFCPOLYLOOP((#4,#2,#3));
#21=IFCFACEOUTERBOUND(#11,.T.);
#22=IFCFACEOUTERBOUND(#12,.T.);
#31=IFCFACE((#21));
#32=IFCFACE((#22));
#40=IFCOPENSHELL((#31));
#41=IFCOPENSHELL((#32));
#50=IFCSHELLBASEDSURFACEMODEL((#40));
#51=IFCSHELLBASEDSURFACEMODEL((#41));
";
    let index = StepIndex::parse(ifc);

    assert!(geometry::item_uses_projected_coordinates(&index, 50));
    assert!(!geometry::item_uses_projected_coordinates(&index, 51));
}

#[test]
fn median_origin_ignores_far_away_feature_outliers() {
    let origin = median_origin(&[
        [292100.0, 2785200.0, 10.0],
        [292120.0, 2785220.0, 12.0],
        [292140.0, 2785240.0, 14.0],
        [2774943.0, 377989.0, -6.0],
        [1174018.0, -2542020.0, 17.0],
    ]);

    assert_eq!(origin.x, 292140.0);
    assert_eq!(origin.y, 2785200.0);
    assert_eq!(origin.z, 12.0);
}

#[test]
fn smooth_normals_average_same_batch_vertices_only() {
    let mut mesh = geometry::Mesh::new();
    mesh.positions = vec![
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, 0.0],
    ];
    mesh.normals = vec![
        [0.0, 0.0, 1.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
    ];
    mesh.colors = vec![[1.0, 1.0, 1.0, 1.0]; mesh.positions.len()];
    mesh.batch_ids = vec![0, 0, 0, 0, 0, 0, 1];

    let smoothed = mesh.with_smoothed_normals_by_position(1e-6);
    let expected = 1.0 / 2.0_f64.sqrt();

    assert!((smoothed.normals[0][0] - expected).abs() < 1e-9);
    assert!((smoothed.normals[0][2] - expected).abs() < 1e-9);
    assert_eq!(smoothed.normals[0], smoothed.normals[3]);
    assert_eq!(smoothed.normals[6], [0.0, 1.0, 0.0]);
}

#[test]
fn smooth_normals_respect_angle_threshold() {
    let mut mesh = geometry::Mesh::new();
    mesh.positions = vec![[0.0, 0.0, 0.0], [0.0, 0.0, 0.0]];
    mesh.normals = vec![[0.0, 0.0, 1.0], [1.0, 0.0, 0.0]];
    mesh.colors = vec![[1.0, 1.0, 1.0, 1.0]; mesh.positions.len()];
    mesh.batch_ids = vec![0, 0];

    let sharp = mesh.with_smoothed_normals_by_position_angle(1e-6, 45.0);
    let full = mesh.with_smoothed_normals_by_position_angle(1e-6, 180.0);

    assert_eq!(sharp.normals[0], [0.0, 0.0, 1.0]);
    assert_eq!(sharp.normals[1], [1.0, 0.0, 0.0]);
    assert_ne!(full.normals[0], mesh.normals[0]);
    assert_eq!(full.normals[0], full.normals[1]);
}
