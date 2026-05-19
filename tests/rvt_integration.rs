use std::{env, path::PathBuf, time::Duration};

use ifc_to_3dtiles::{
    convert::{ConvertOptions, NormalMode},
    revit::detect_revit_installations,
    rvt::{RvtToIfcOptions, export_rvt_to_ifc},
};

#[test]
fn gated_rvt_to_ifc_to_glb_integration() {
    let Some(sample_rvt) = env::var_os("RVT_TO_GLB_SAMPLE_RVT").map(PathBuf::from) else {
        eprintln!("skip: RVT_TO_GLB_SAMPLE_RVT 未設定");
        return;
    };
    if !sample_rvt.is_file() {
        eprintln!("skip: sample RVT 不存在：{}", sample_rvt.display());
        return;
    }
    if detect_revit_installations().is_empty() {
        eprintln!("skip: 未偵測到 Revit 2025/2026/2027");
        return;
    }

    let bridge = env::var_os("RVT_TO_GLB_BRIDGE").map(PathBuf::from);
    if let Some(path) = &bridge
        && !path.is_file()
    {
        eprintln!("skip: RVT_TO_GLB_BRIDGE 不存在：{}", path.display());
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let ifc = temp.path().join("sample.ifc");
    let exported = export_rvt_to_ifc(&RvtToIfcOptions {
        input_rvt: sample_rvt,
        output_ifc: ifc.clone(),
        requested_version: None,
        revit_exe: None,
        bridge_assembly: bridge,
        timeout: Duration::from_secs(30 * 60),
    })
    .expect("rvt export");
    assert!(exported.is_file());

    let outputs = ifc_to_3dtiles::convert_path(&ConvertOptions {
        input: ifc,
        output: temp.path().join("out"),
        source_epsg: 3826,
        tile_max_features: 100,
        tile_max_triangles: 1000,
        normal_mode: NormalMode::Both,
        smooth_angle_deg: 90.0,
        overwrite: true,
    })
    .expect("ifc convert");
    let out_dir = outputs.first().expect("output dir");
    assert!(out_dir.join("sample_flat.glb").is_file());
    assert!(out_dir.join("sample_smooth.glb").is_file());
    assert!(out_dir.join("metadata.json").is_file());
}
