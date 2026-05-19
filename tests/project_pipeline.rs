use ifc_to_3dtiles::{
    cad_metadata::{CadHierarchyDump, CadLevel, CadMaterial, CadModel, CadReference},
    fingerprint::{GeometryFingerprint, duplicate_candidate_score},
    georef::{
        Aoi, Bounds2, BoundsSummary, SourceTransform, classify_source_scale, decide_source_status,
    },
    inspect::{discover_sources, source_format_from_path, write_empty_cad_metadata_dumps},
    project::{ProjectManifest, SourceFormat, SourceRecord, SourceStatus, WorkspaceLayout},
};
use std::{fs, path::PathBuf};

#[test]
fn workspace_layout_uses_predictable_folders() {
    let layout = WorkspaceLayout::new(PathBuf::from(r"C:\work\tamkang_bridge"));

    assert_eq!(
        layout.sources,
        PathBuf::from(r"C:\work\tamkang_bridge\sources")
    );
    assert_eq!(
        layout.staging,
        PathBuf::from(r"C:\work\tamkang_bridge\staging")
    );
    assert_eq!(
        layout.normalized,
        PathBuf::from(r"C:\work\tamkang_bridge\normalized")
    );
    assert_eq!(
        layout.publish,
        PathBuf::from(r"C:\work\tamkang_bridge\publish")
    );
}

#[test]
fn source_record_preserves_source_identity() {
    let source = SourceRecord {
        id: "dgn-djb-m-su-monitor".to_string(),
        path: PathBuf::from(r"sources\DJB-M-SU-監測.dgn.i.dgn"),
        format: SourceFormat::Dgn,
        status: SourceStatus::PendingInspect,
        original_size_bytes: 170_884_000,
        detected_crs: None,
        unit_scale_to_meter: None,
        anchor_distance_m: None,
        raw_bbox: None,
        percentile_bbox: None,
        transform: None,
        cad_metadata_path: None,
        fingerprint_hash: None,
        duplicate_candidates: vec![],
        warnings: vec![],
    };

    assert_eq!(source.format, SourceFormat::Dgn);
    assert_eq!(source.status, SourceStatus::PendingInspect);
}

#[test]
fn project_manifest_serializes_to_stable_json() {
    let manifest = ProjectManifest {
        project_id: "tamkang_bridge".to_string(),
        source_epsg: 3826,
        anchor_source_id: None,
        allowed_scales: vec![1000.0, 1.0, 0.1, 0.01, 0.001],
        sources: vec![],
    };
    let json = serde_json::to_string_pretty(&manifest).expect("serialize manifest");

    assert!(json.contains("\"source_epsg\": 3826"));
    assert!(json.contains("\"allowed_scales\""));
    assert!(json.contains("1000.0"));
    assert!(json.contains("0.001"));
}

#[test]
fn source_format_detects_supported_extensions() {
    assert_eq!(source_format_from_path("bridge.ifc"), SourceFormat::Ifc);
    assert_eq!(source_format_from_path("bridge.RVT"), SourceFormat::Rvt);
    assert_eq!(
        source_format_from_path("bridge.dgn.i.dgn"),
        SourceFormat::Dgn
    );
    assert_eq!(source_format_from_path("bridge.DWG"), SourceFormat::Dwg);
    assert_eq!(source_format_from_path("readme.txt"), SourceFormat::Unknown);
}

#[test]
fn discover_sources_skips_unknown_files_and_keeps_large_cad_out_of_git() {
    let tmp = tempfile::tempdir().expect("tempdir");
    fs::write(tmp.path().join("bridge.ifc"), "ifc").expect("ifc");
    fs::write(tmp.path().join("bridge.dgn.i.dgn"), "dgn").expect("dgn");
    fs::write(tmp.path().join("bridge.dwg"), "dwg").expect("dwg");
    fs::write(tmp.path().join("notes.txt"), "skip").expect("txt");

    let sources = discover_sources(tmp.path()).expect("discover sources");
    let formats: Vec<_> = sources.iter().map(|source| source.format).collect();

    assert_eq!(sources.len(), 3);
    assert!(formats.contains(&SourceFormat::Ifc));
    assert!(formats.contains(&SourceFormat::Dgn));
    assert!(formats.contains(&SourceFormat::Dwg));
    assert!(
        sources
            .iter()
            .all(|source| source.status == SourceStatus::PendingInspect)
    );
    assert!(sources.iter().all(|source| source.id.is_ascii()));
}

#[test]
fn discover_sources_generates_unique_ids_for_non_ascii_names() {
    let tmp = tempfile::tempdir().expect("tempdir");
    fs::write(tmp.path().join("主橋.dwg"), "dwg").expect("dwg");
    fs::write(tmp.path().join("主橋塔.dwg"), "dwg").expect("dwg");
    fs::write(tmp.path().join("管理中心_全.dwg"), "dwg").expect("dwg");

    let sources = discover_sources(tmp.path()).expect("discover sources");
    let mut ids: Vec<_> = sources.iter().map(|source| source.id.clone()).collect();
    ids.sort();
    ids.dedup();

    assert_eq!(sources.len(), 3);
    assert_eq!(ids.len(), 3);
    assert!(sources.iter().all(|source| source.id.is_ascii()));
}

#[test]
fn scale_classifier_accepts_taiwan_epsg_3826_meter_bounds() {
    let aoi = Aoi::new(120_000.0, 2_400_000.0, 360_000.0, 2_800_000.0);
    let bounds = Bounds2::new(300_000.0, 2_787_000.0, 301_000.0, 2_788_000.0);
    let summary = BoundsSummary::from_raw_and_percentile(bounds, bounds);
    let result = classify_source_scale(&summary, &aoi, &[1000.0, 1.0, 0.1, 0.01, 0.001]);

    assert_eq!(result.selected_scale, Some(1.0));
    assert_eq!(result.status, "inside_aoi");
}

#[test]
fn scale_classifier_detects_centimeter_like_coordinates() {
    let aoi = Aoi::new(120_000.0, 2_400_000.0, 360_000.0, 2_800_000.0);
    let bounds = Bounds2::new(30_000_000.0, 278_700_000.0, 30_100_000.0, 278_800_000.0);
    let summary = BoundsSummary::from_raw_and_percentile(bounds, bounds);
    let result = classify_source_scale(&summary, &aoi, &[1000.0, 1.0, 0.1, 0.01, 0.001]);

    assert_eq!(result.selected_scale, Some(0.01));
    assert_eq!(result.status, "inside_aoi");
}

#[test]
fn scale_classifier_detects_millimeter_like_coordinates() {
    let aoi = Aoi::new(120_000.0, 2_400_000.0, 360_000.0, 2_800_000.0);
    let bounds = Bounds2::new(
        300_000_000.0,
        2_787_000_000.0,
        301_000_000.0,
        2_788_000_000.0,
    );
    let summary = BoundsSummary::from_raw_and_percentile(bounds, bounds);
    let result = classify_source_scale(&summary, &aoi, &[1000.0, 1.0, 0.1, 0.01, 0.001]);

    assert_eq!(result.selected_scale, Some(0.001));
    assert_eq!(result.status, "inside_aoi");
}

#[test]
fn scale_classifier_uses_percentile_bounds_when_raw_bbox_has_stray_points() {
    let aoi = Aoi::new(120_000.0, 2_400_000.0, 360_000.0, 2_800_000.0);
    let raw_bounds = Bounds2::new(-9_000_000.0, -9_000_000.0, 9_000_000.0, 9_000_000.0);
    let percentile_bounds = Bounds2::new(300_000.0, 2_787_000.0, 301_000.0, 2_788_000.0);
    let summary = BoundsSummary::from_raw_and_percentile(raw_bounds, percentile_bounds);
    let result = classify_source_scale(&summary, &aoi, &[1000.0, 1.0, 0.1, 0.01, 0.001]);

    assert_eq!(result.selected_scale, Some(1.0));
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("raw bbox"))
    );
}

#[test]
fn scale_classifier_quarantines_far_away_model() {
    let aoi = Aoi::new(120_000.0, 2_400_000.0, 360_000.0, 2_800_000.0);
    let bounds = Bounds2::new(4_000_000.0, 6_000_000.0, 4_100_000.0, 6_100_000.0);
    let summary = BoundsSummary::from_raw_and_percentile(bounds, bounds);
    let result = classify_source_scale(&summary, &aoi, &[1000.0, 1.0, 0.1, 0.01, 0.001]);

    assert_eq!(result.selected_scale, None);
    assert_eq!(result.status, "outside_aoi");
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("outside AOI"))
    );
}

#[test]
fn source_transform_declares_canonical_space() {
    let transform = SourceTransform::identity("EPSG:3826", 1.0);

    assert_eq!(transform.canonical_crs, "EPSG:3826");
    assert_eq!(
        transform.canonical_space,
        "EPSG:3826 meters / local ENU / Z-up"
    );
    assert_eq!(transform.scale, [1.0, 1.0, 1.0]);
}

#[test]
fn decide_source_status_approves_inside_aoi_3d_source() {
    let aoi = Aoi::new(120_000.0, 2_400_000.0, 360_000.0, 2_800_000.0);
    let bounds = Bounds2::new(300_000.0, 2_787_000.0, 301_000.0, 2_788_000.0);
    let summary = BoundsSummary::from_raw_and_percentile(bounds, bounds);
    let status = decide_source_status(summary, 20.0, &aoi, &[1000.0, 1.0, 0.1, 0.01, 0.001]);

    assert_eq!(status.status, SourceStatus::Approved);
    assert_eq!(status.selected_scale, Some(1.0));
}

#[test]
fn decide_source_status_quarantines_flat_2d_source() {
    let aoi = Aoi::new(120_000.0, 2_400_000.0, 360_000.0, 2_800_000.0);
    let bounds = Bounds2::new(300_000.0, 2_787_000.0, 301_000.0, 2_788_000.0);
    let summary = BoundsSummary::from_raw_and_percentile(bounds, bounds);
    let status = decide_source_status(summary, 0.001, &aoi, &[1000.0, 1.0, 0.1, 0.01, 0.001]);

    assert_eq!(status.status, SourceStatus::Quarantined);
    assert!(status.warnings.iter().any(|warning| warning.contains("2D")));
}

#[test]
fn geometry_fingerprint_detects_probable_duplicate_sources() {
    let a = GeometryFingerprint {
        source_id: "bridge-ifc".to_string(),
        vertex_count: 100_000,
        triangle_count: 180_000,
        bbox: [300_000.0, 2_787_000.0, 0.0, 301_000.0, 2_788_000.0, 60.0],
        surface_area_m2: 125_000.0,
        hash: "a".to_string(),
    };
    let b = GeometryFingerprint {
        source_id: "bridge-dgn".to_string(),
        vertex_count: 100_250,
        triangle_count: 179_900,
        bbox: [300_000.2, 2_787_000.1, 0.0, 301_000.1, 2_788_000.2, 60.1],
        surface_area_m2: 125_100.0,
        hash: "b".to_string(),
    };

    let score = duplicate_candidate_score(&a, &b);
    assert!(score >= 0.95);
}

#[test]
fn cad_hierarchy_dump_preserves_dgn_metadata_buckets() {
    let dump = CadHierarchyDump {
        source_id: "bridge-dgn".to_string(),
        models: vec![CadModel {
            name: "Default".to_string(),
            element_count: 120,
        }],
        references: vec![CadReference {
            name: "pier-ref".to_string(),
            path: "pier.dgn".to_string(),
        }],
        levels: vec![CadLevel {
            name: "Cable".to_string(),
            element_count: 80,
        }],
        cells: vec![],
        shared_cells: vec![],
        attachments: vec![],
        element_classes: vec!["Primary".to_string()],
        materials: vec![CadMaterial {
            name: "concrete".to_string(),
            color_rgba: [0.8, 0.8, 0.78, 1.0],
        }],
        line_styles: vec!["ByLevel".to_string()],
        warnings: vec![],
    };

    let json = serde_json::to_value(&dump).expect("serialize CAD dump");
    assert!(json.get("models").is_some());
    assert!(json.get("references").is_some());
    assert!(json.get("levels").is_some());
    assert!(json.get("cells").is_some());
    assert!(json.get("shared_cells").is_some());
    assert!(json.get("attachments").is_some());
    assert!(json.get("element_classes").is_some());
    assert!(json.get("materials").is_some());
    assert!(json.get("line_styles").is_some());
}

#[test]
fn inspect_writes_empty_cad_metadata_sidecars_for_dgn_and_dwg() {
    let tmp = tempfile::tempdir().expect("tempdir");
    fs::write(tmp.path().join("a.dgn"), "dgn").expect("dgn");
    fs::write(tmp.path().join("b.dwg"), "dwg").expect("dwg");
    fs::write(tmp.path().join("c.ifc"), "ifc").expect("ifc");

    let mut sources = discover_sources(tmp.path()).expect("discover sources");
    let output = tmp.path().join("out");
    write_empty_cad_metadata_dumps(&mut sources, &output).expect("write CAD metadata");

    let cad_sources: Vec<_> = sources
        .iter()
        .filter(|source| matches!(source.format, SourceFormat::Dgn | SourceFormat::Dwg))
        .collect();
    assert_eq!(cad_sources.len(), 2);
    for source in cad_sources {
        let relative_path = source
            .cad_metadata_path
            .as_ref()
            .expect("cad metadata path");
        let metadata_path = output.join(relative_path);
        assert!(metadata_path.exists());

        let dump: CadHierarchyDump =
            serde_json::from_slice(&fs::read(&metadata_path).expect("read metadata sidecar"))
                .expect("parse metadata sidecar");
        assert_eq!(dump.source_id, source.id);
        assert!(dump.models.is_empty());
        assert!(
            dump.warnings
                .iter()
                .any(|warning| warning.contains("CAD hierarchy probe unavailable"))
        );
    }

    let ifc_source = sources
        .iter()
        .find(|source| source.format == SourceFormat::Ifc)
        .expect("IFC source");
    assert!(ifc_source.cad_metadata_path.is_none());
}

#[test]
fn inspect_removes_stale_cad_metadata_sidecars_before_writing() {
    let tmp = tempfile::tempdir().expect("tempdir");
    fs::write(tmp.path().join("a.dgn"), "dgn").expect("dgn");

    let mut sources = discover_sources(tmp.path()).expect("discover sources");
    let output = tmp.path().join("out");
    let stale_dir = output.join("cad_metadata");
    fs::create_dir_all(&stale_dir).expect("stale dir");
    fs::write(stale_dir.join("stale.json"), "{}").expect("stale file");

    write_empty_cad_metadata_dumps(&mut sources, &output).expect("write CAD metadata");

    let sidecars: Vec<_> = fs::read_dir(&stale_dir)
        .expect("read sidecars")
        .map(|entry| entry.expect("entry").file_name())
        .collect();

    assert_eq!(sidecars.len(), 1);
    assert_ne!(sidecars[0], "stale.json");
}
