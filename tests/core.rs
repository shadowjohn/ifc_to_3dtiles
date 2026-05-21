use std::path::Path;

use ifc_to_3dtiles::{
    b3dm,
    convert::{ConvertOptions, NormalMode, median_origin},
    crs,
    geometry::{self, MeshBuildOptions},
    model::StyleTable,
    revit::{
        RevitVersion, detect_revit_installations_in_roots, missing_revit_installation_message,
        revit_installation_from_exe,
    },
    rvt_job::{RvtExportJob, RvtExportOptions},
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
fn face_based_surface_model_generates_triangles() {
    let ifc = "\
#1=IFCCARTESIANPOINT((0.,0.,0.));
#2=IFCCARTESIANPOINT((1.,0.,0.));
#3=IFCCARTESIANPOINT((1.,1.,0.));
#4=IFCCARTESIANPOINT((0.,1.,0.));
#11=IFCPOLYLOOP((#1,#2,#3,#4));
#21=IFCFACEOUTERBOUND(#11,.T.);
#31=IFCFACE((#21));
#40=IFCCONNECTEDFACESET((#31));
#50=IFCFACEBASEDSURFACEMODEL((#40));
";
    let index = StepIndex::parse(ifc);
    let mesh = geometry::mesh_face_based_surface_model(
        &index,
        50,
        &geometry::Mat4::identity(),
        MeshBuildOptions {
            batch_id: 4,
            color: [0.0, 0.0, 1.0, 1.0],
        },
    )
    .expect("face based surface model mesh");

    assert_eq!(mesh.triangle_count(), 2);
    assert!(mesh.batch_ids.iter().all(|id| *id == 4));
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

#[test]
fn revit_detection_finds_supported_install_roots() {
    let temp = tempfile::tempdir().expect("tempdir");
    let autodesk = temp.path().join("Autodesk");
    let revit_2026 = autodesk.join("Revit 2026");
    std::fs::create_dir_all(&revit_2026).expect("create fake revit dir");
    std::fs::write(revit_2026.join("Revit.exe"), b"").expect("fake revit exe");
    std::fs::create_dir_all(autodesk.join("Revit 2024")).expect("unsupported fake revit dir");
    std::fs::write(autodesk.join("Revit 2024").join("Revit.exe"), b"")
        .expect("fake unsupported revit exe");

    let installs = detect_revit_installations_in_roots([autodesk.as_path()]);

    assert_eq!(installs.len(), 1);
    assert_eq!(installs[0].version, RevitVersion::V2026);
    assert_eq!(installs[0].revit_exe, revit_2026.join("Revit.exe"));
}

#[test]
fn revit_detection_accepts_release_named_install_dirs() {
    let temp = tempfile::tempdir().expect("tempdir");
    let autodesk = temp.path().join("Autodesk");
    let revit_2027 = autodesk.join("Revit 2027 Release");
    let revit_2025 = autodesk.join("Autodesk Revit 2025");
    std::fs::create_dir_all(&revit_2027).expect("create fake revit dir");
    std::fs::create_dir_all(&revit_2025).expect("create fake revit dir");
    std::fs::write(revit_2027.join("Revit.exe"), b"").expect("fake revit exe");
    std::fs::write(revit_2025.join("Revit.exe"), b"").expect("fake revit exe");

    let installs = detect_revit_installations_in_roots([autodesk.as_path()]);

    assert_eq!(installs.len(), 2);
    assert_eq!(installs[0].version, RevitVersion::V2027);
    assert_eq!(installs[0].revit_exe, revit_2027.join("Revit.exe"));
    assert_eq!(installs[1].version, RevitVersion::V2025);
    assert_eq!(installs[1].revit_exe, revit_2025.join("Revit.exe"));
}

#[test]
fn explicit_revit_exe_parses_supported_version_from_path() {
    let temp = tempfile::tempdir().expect("tempdir");
    let install_dir = temp.path().join("Custom Autodesk").join("Revit 2026");
    std::fs::create_dir_all(&install_dir).expect("create fake revit dir");
    let revit_exe = install_dir.join("Revit.exe");
    std::fs::write(&revit_exe, b"").expect("fake revit exe");

    let install = revit_installation_from_exe(&revit_exe, None).expect("explicit install");

    assert_eq!(install.version, RevitVersion::V2026);
    assert_eq!(install.install_dir, install_dir);
    assert_eq!(install.revit_exe, revit_exe);
}

#[test]
fn missing_revit_message_points_to_official_downloads_and_manual_path() {
    let message = missing_revit_installation_message();

    assert!(message.contains("https://manage.autodesk.com/products"));
    assert!(message.contains("https://www.autodesk.com/products/revit/free-trial"));
    assert!(message.contains("--revit-exe"));
}

#[test]
fn revit_bridge_ifc_export_is_wrapped_in_transaction() {
    let source = include_str!("../revit_bridge/ExportApplication.cs");

    assert!(source.contains("new Transaction(document"));
    assert!(source.contains("transaction.Start()"));
    assert!(source.contains("transaction.Commit()"));
}

#[test]
fn rvt_export_job_serializes_expected_paths_and_ifc_options() {
    let job = RvtExportJob {
        input_rvt: "C:\\models\\damper.rvt".into(),
        output_ifc: "C:\\out\\damper.ifc".into(),
        result_json: "C:\\out\\damper.rvt-export-result.json".into(),
        options: RvtExportOptions::default(),
    };

    let value = serde_json::to_value(&job).expect("serialize job");

    assert_eq!(value["inputRvt"], "C:\\models\\damper.rvt");
    assert_eq!(value["outputIfc"], "C:\\out\\damper.ifc");
    assert_eq!(value["options"]["fileVersion"], "IFC2x3CV2");
    assert_eq!(value["options"]["exportInternalRevitPropertySets"], true);
    assert_eq!(value["options"]["exportBaseQuantities"], true);
    assert_eq!(value["options"]["exportMaterialPsets"], true);
}

#[test]
fn revit_like_ifc_wall_converts_to_flat_and_smooth_glb_with_metadata() {
    let temp = tempfile::tempdir().expect("tempdir");
    let input = temp.path().join("revit_wall.ifc");
    std::fs::write(&input, revit_like_ifc_wall()).expect("write ifc fixture");
    let output = temp.path().join("out");

    let outputs = ifc_to_3dtiles::convert_path(&ConvertOptions {
        input,
        output: output.clone(),
        source_epsg: 3826,
        tile_max_features: 100,
        tile_max_triangles: 1000,
        normal_mode: NormalMode::Both,
        smooth_angle_deg: 90.0,
        overwrite: true,
    })
    .expect("convert revit-like ifc");

    let out_dir = outputs.first().expect("output dir");
    assert!(out_dir.join("revit_wall_flat.glb").is_file());
    assert!(out_dir.join("revit_wall_smooth.glb").is_file());
    assert!(out_dir.join("metadata.json").is_file());
    assert!(out_dir.join("unsupported_geometry_report.json").is_file());
    assert!(out_dir.join("ifc_info.html").is_file());
    assert!(out_dir.join("ifc_info.json").is_file());
    assert!(out_dir.join("ifc_products.csv").is_file());
    assert!(out_dir.join("ifc_properties.csv").is_file());
    assert!(out_dir.join("ifc_geometry_items.csv").is_file());
    assert!(out_dir.join("tileset_smooth_90.json").is_file());
    assert!(
        out_dir
            .join("tiles_smooth_90")
            .join("tile_0000.b3dm")
            .is_file()
    );

    let flat_glb_json = read_glb_json(out_dir.join("revit_wall_flat.glb"));
    assert_eq!(
        flat_glb_json["nodes"][0]["extras"]["features"][0]["ifc_type"],
        "IFCWALL"
    );
    assert_eq!(flat_glb_json["meshes"][0]["extras"]["normalMode"], "flat");

    let metadata: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out_dir.join("metadata.json")).expect("metadata"))
            .expect("metadata json");
    assert_eq!(metadata[0]["ifc_type"], "IFCWALL");
    assert_eq!(metadata[0]["name"], "Basic Wall");
    assert_eq!(metadata[0]["psets"]["Pset_WallCommon"]["Reference"], "W1");

    let info_html = std::fs::read_to_string(out_dir.join("ifc_info.html")).expect("info html");
    assert!(info_html.contains("Basic Wall"));
    assert!(info_html.contains("Pset_WallCommon"));
    assert!(info_html.contains("IFCFACETEDBREP"));

    let products_csv =
        std::fs::read_to_string(out_dir.join("ifc_products.csv")).expect("products csv");
    assert!(products_csv.contains("ifc_step_id,global_id,ifc_type"));
    assert!(products_csv.contains("90,WALLGUID,IFCWALL"));
    assert!(products_csv.contains(",true,12"));

    let properties_csv =
        std::fs::read_to_string(out_dir.join("ifc_properties.csv")).expect("properties csv");
    assert!(properties_csv.contains("90,WALLGUID,IFCWALL"));
    assert!(properties_csv.contains("Pset_WallCommon,Reference,W1"));
    assert!(properties_csv.contains("Pset_WallCommon,DangerousFormula,'=2+2"));

    let geometry_csv =
        std::fs::read_to_string(out_dir.join("ifc_geometry_items.csv")).expect("geometry csv");
    assert!(geometry_csv.contains("90,WALLGUID,IFCWALL"));
    assert!(geometry_csv.contains("Body,Brep,61,IFCFACETEDBREP"));

    let smooth_90_tileset: serde_json::Value = serde_json::from_slice(
        &std::fs::read(out_dir.join("tileset_smooth_90.json")).expect("smooth 90 tileset"),
    )
    .expect("smooth 90 tileset json");
    assert_eq!(
        smooth_90_tileset["root"]["children"][0]["content"]["uri"],
        "tiles_smooth_90/tile_0000.b3dm"
    );
}

#[test]
fn swept_solid_circle_proxy_converts_to_glb_with_metadata() {
    let temp = tempfile::tempdir().expect("tempdir");
    let input = temp.path().join("revit_cable.ifc");
    std::fs::write(&input, revit_like_ifc_cable_swept_solid()).expect("write ifc fixture");
    let output = temp.path().join("out");

    let outputs = ifc_to_3dtiles::convert_path(&ConvertOptions {
        input,
        output: output.clone(),
        source_epsg: 3826,
        tile_max_features: 100,
        tile_max_triangles: 1000,
        normal_mode: NormalMode::Both,
        smooth_angle_deg: 90.0,
        overwrite: true,
    })
    .expect("convert swept solid cable");

    let out_dir = outputs.first().expect("output dir");
    let flat_glb_json = read_glb_json(out_dir.join("revit_cable_flat.glb"));
    assert_eq!(flat_glb_json["accessors"][0]["count"], 288);
    assert_eq!(
        flat_glb_json["nodes"][0]["extras"]["features"][0]["ifc_type"],
        "IFCBUILDINGELEMENTPROXY"
    );

    let metadata: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out_dir.join("metadata.json")).expect("metadata"))
            .expect("metadata json");
    assert_eq!(metadata[0]["name"], "P44橋柱預力鋼纜");
}

#[test]
fn unsupported_report_includes_skipped_empty_products() {
    let temp = tempfile::tempdir().expect("tempdir");
    let input = temp.path().join("unsupported_swept.ifc");
    std::fs::write(&input, revit_like_ifc_wall_with_unsupported_swept_solid())
        .expect("write ifc fixture");
    let output = temp.path().join("out");

    let outputs = ifc_to_3dtiles::convert_path(&ConvertOptions {
        input,
        output: output.clone(),
        source_epsg: 3826,
        tile_max_features: 100,
        tile_max_triangles: 1000,
        normal_mode: NormalMode::Both,
        smooth_angle_deg: 90.0,
        overwrite: true,
    })
    .expect("convert fixture with unsupported swept solid");

    let out_dir = outputs.first().expect("output dir");
    let report: serde_json::Value = serde_json::from_slice(
        &std::fs::read(out_dir.join("unsupported_geometry_report.json"))
            .expect("unsupported report"),
    )
    .expect("unsupported report json");
    assert_eq!(report["unsupported_items"]["IFCEXTRUDEDAREASOLID"], 1);
}

#[test]
fn ifc_info_subcommand_core_exports_html_and_csv_without_converting_tiles() {
    let temp = tempfile::tempdir().expect("tempdir");
    let input = temp.path().join("revit_wall.ifc");
    std::fs::write(&input, revit_like_ifc_wall()).expect("write ifc fixture");
    let output = temp.path().join("ifc_info");

    let outputs =
        ifc_to_3dtiles::ifc_info::write_ifc_info_path(&input, &output).expect("write ifc info");

    assert_eq!(outputs, vec![output.clone()]);
    assert!(output.join("ifc_info.html").is_file());
    assert!(output.join("ifc_products.csv").is_file());
    assert!(output.join("ifc_properties.csv").is_file());
    assert!(output.join("ifc_geometry_items.csv").is_file());
    assert!(!output.join("tileset.json").exists());

    let products_csv =
        std::fs::read_to_string(output.join("ifc_products.csv")).expect("products csv");
    assert!(products_csv.contains("90,WALLGUID,IFCWALL"));
    assert!(products_csv.contains(",false,0"));
}

#[test]
fn single_large_feature_is_split_across_multiple_b3dm_tiles() {
    let temp = tempfile::tempdir().expect("tempdir");
    let input = temp.path().join("revit_wall.ifc");
    std::fs::write(&input, revit_like_ifc_wall()).expect("write ifc fixture");
    let output = temp.path().join("out");

    let outputs = ifc_to_3dtiles::convert_path(&ConvertOptions {
        input,
        output: output.clone(),
        source_epsg: 3826,
        tile_max_features: 100,
        tile_max_triangles: 4,
        normal_mode: NormalMode::Both,
        smooth_angle_deg: 90.0,
        overwrite: true,
    })
    .expect("convert split fixture");

    let out_dir = outputs.first().expect("output dir");
    let tile_count = std::fs::read_dir(out_dir.join("tiles"))
        .expect("tiles dir")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "b3dm"))
        .count();
    let smooth_tile_count = std::fs::read_dir(out_dir.join("tiles_smooth"))
        .expect("smooth tiles dir")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "b3dm"))
        .count();

    assert_eq!(tile_count, 3);
    assert_eq!(smooth_tile_count, 3);
    assert!(out_dir.join("tiles").join("tile_0002.b3dm").is_file());
    assert!(
        out_dir
            .join("tiles_smooth")
            .join("tile_0002.b3dm")
            .is_file()
    );
}

fn read_glb_json(path: impl AsRef<Path>) -> serde_json::Value {
    let bytes = std::fs::read(path).expect("read glb");
    assert_eq!(&bytes[0..4], b"glTF");
    let json_length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    assert_eq!(&bytes[16..20], b"JSON");
    serde_json::from_slice(&bytes[20..20 + json_length]).expect("glb json")
}

fn revit_like_ifc_wall() -> &'static str {
    "\
ISO-10303-21;
HEADER;
FILE_DESCRIPTION(('ViewDefinition [CoordinationView_V2.0]'),'2;1');
FILE_SCHEMA(('IFC2X3'));
ENDSEC;
DATA;
#1=IFCPROJECT('P1',$,'Project',$,$,$,$,$);
#2=IFCSITE('S1',$,'Site',$,$,$,$,$,$,$,$,$,$,$);
#3=IFCBUILDING('B1',$,'Building',$,$,$,$,$,$,$,$,$);
#4=IFCBUILDINGSTOREY('ST1',$,'Level 1',$,$,$,$,$,$);
#5=IFCRELAGGREGATES('RA1',$,$,$,#1,(#2));
#6=IFCRELAGGREGATES('RA2',$,$,$,#2,(#3));
#7=IFCRELAGGREGATES('RA3',$,$,$,#3,(#4));
#10=IFCCARTESIANPOINT((0.,0.,0.));
#11=IFCDIRECTION((0.,0.,1.));
#12=IFCDIRECTION((1.,0.,0.));
#13=IFCAXIS2PLACEMENT3D(#10,#11,#12);
#14=IFCLOCALPLACEMENT($,#13);
#20=IFCCARTESIANPOINT((0.,0.,0.));
#21=IFCCARTESIANPOINT((1.,0.,0.));
#22=IFCCARTESIANPOINT((1.,1.,0.));
#23=IFCCARTESIANPOINT((0.,1.,0.));
#24=IFCCARTESIANPOINT((0.,0.,1.));
#25=IFCCARTESIANPOINT((1.,0.,1.));
#26=IFCCARTESIANPOINT((1.,1.,1.));
#27=IFCCARTESIANPOINT((0.,1.,1.));
#30=IFCPOLYLOOP((#20,#21,#22,#23));
#31=IFCPOLYLOOP((#24,#27,#26,#25));
#32=IFCPOLYLOOP((#20,#24,#25,#21));
#33=IFCPOLYLOOP((#21,#25,#26,#22));
#34=IFCPOLYLOOP((#22,#26,#27,#23));
#35=IFCPOLYLOOP((#23,#27,#24,#20));
#40=IFCFACEOUTERBOUND(#30,.T.);
#41=IFCFACEOUTERBOUND(#31,.T.);
#42=IFCFACEOUTERBOUND(#32,.T.);
#43=IFCFACEOUTERBOUND(#33,.T.);
#44=IFCFACEOUTERBOUND(#34,.T.);
#45=IFCFACEOUTERBOUND(#35,.T.);
#50=IFCFACE((#40));
#51=IFCFACE((#41));
#52=IFCFACE((#42));
#53=IFCFACE((#43));
#54=IFCFACE((#44));
#55=IFCFACE((#45));
#60=IFCCLOSEDSHELL((#50,#51,#52,#53,#54,#55));
#61=IFCFACETEDBREP(#60);
#70=IFCCOLOURRGB($,0.1,0.2,0.3);
#71=IFCSURFACESTYLESHADING(#70);
#72=IFCSURFACESTYLE($,.BOTH.,(#71));
#73=IFCPRESENTATIONSTYLEASSIGNMENT((#72));
#74=IFCSTYLEDITEM(#61,(#73),$);
#80=IFCSHAPEREPRESENTATION($,'Body','Brep',(#61));
#81=IFCPRODUCTDEFINITIONSHAPE($,$,(#80));
#90=IFCWALL('WALLGUID',$,'Basic Wall','Wall from Revit',$,#14,#81,'W1');
#91=IFCRELCONTAINEDINSPATIALSTRUCTURE('RC1',$,$,$,(#90),#4);
#100=IFCPROPERTYSINGLEVALUE('Reference',$,IFCLABEL('W1'),$);
#103=IFCPROPERTYSINGLEVALUE('DangerousFormula',$,IFCLABEL('=2+2'),$);
#101=IFCPROPERTYSET('PS1',$,'Pset_WallCommon',$,(#100,#103));
#102=IFCRELDEFINESBYPROPERTIES('RD1',$,$,$,(#90),#101);
ENDSEC;
END-ISO-10303-21;
"
}

fn revit_like_ifc_cable_swept_solid() -> &'static str {
    "\
ISO-10303-21;
HEADER;
FILE_DESCRIPTION(('ViewDefinition [CoordinationView_V2.0]'),'2;1');
FILE_SCHEMA(('IFC2X3'));
ENDSEC;
DATA;
#1=IFCPROJECT('P1',$,'Project',$,$,$,$,$);
#10=IFCCARTESIANPOINT((0.,0.,0.));
#11=IFCDIRECTION((0.,0.,1.));
#12=IFCDIRECTION((1.,0.,0.));
#13=IFCAXIS2PLACEMENT3D(#10,#11,#12);
#14=IFCLOCALPLACEMENT($,#13);
#20=IFCCARTESIANPOINT((0.,0.));
#21=IFCDIRECTION((1.,0.));
#22=IFCAXIS2PLACEMENT2D(#20,#21);
#23=IFCCIRCLEPROFILEDEF(.AREA.,'Cable',#22,0.5);
#24=IFCEXTRUDEDAREASOLID(#23,#13,#11,10.);
#25=IFCSHAPEREPRESENTATION($,'Body','SweptSolid',(#24));
#26=IFCPRODUCTDEFINITIONSHAPE($,$,(#25));
#27=IFCBUILDINGELEMENTPROXY('CABLEGUID',$,'P44\\X2\\6A4B67F19810529B92FC7E9C\\X0\\',$,$,#14,#26,'C1',$);
ENDSEC;
END-ISO-10303-21;
"
}

fn revit_like_ifc_wall_with_unsupported_swept_solid() -> String {
    revit_like_ifc_wall().replace(
        "ENDSEC;\nEND-ISO-10303-21;",
        "\
#200=IFCCARTESIANPOINT((0.,0.));
#201=IFCCARTESIANPOINT((1.,0.));
#202=IFCCARTESIANPOINT((0.,1.));
#203=IFCPOLYLINE((#200,#201,#202,#200));
#204=IFCARBITRARYCLOSEDPROFILEDEF(.AREA.,'Triangle',#203);
#205=IFCEXTRUDEDAREASOLID(#204,#13,#11,1.);
#206=IFCSHAPEREPRESENTATION($,'Body','SweptSolid',(#205));
#207=IFCPRODUCTDEFINITIONSHAPE($,$,(#206));
#208=IFCBUILDINGELEMENTPROXY('UNSUPPORTEDGUID',$,'Unsupported swept',$,$,#14,#207,'U1',$);
ENDSEC;
END-ISO-10303-21;",
    )
}
