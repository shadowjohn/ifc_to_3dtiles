use ifc_to_3dtiles::{
    georef::{Aoi, Bounds2, BoundsSummary, SourceTransform, classify_source_scale},
    inspect::{discover_sources, source_format_from_path},
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
