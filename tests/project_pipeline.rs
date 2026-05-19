use ifc_to_3dtiles::{
    cad_conversion::{
        CadConversionReportEntry, CadConversionStatus, NormalizedCadInspectReportEntry,
    },
    cad_entity_inspect::{
        CadEntity, CadEntityStats, entity_fingerprint_hash, parse_ogrinfo_entities,
        scan_wkt_coordinates, summarize_entities, write_entity_inspect_db,
    },
    cad_metadata::{CadHierarchyDump, CadLevel, CadMaterial, CadModel, CadReference},
    fingerprint::{GeometryFingerprint, duplicate_candidate_score},
    georef::{
        Aoi, Bounds2, BoundsSummary, SourceTransform, classify_source_scale, decide_source_status,
    },
    inspect::{CadProbeSummary, read_cad_probe_summary},
    inspect::{discover_sources, source_format_from_path, write_empty_cad_metadata_dumps},
    inspect_drilldown::{
        ApprovalSourceDecision, EntityBboxRecord, classify_approval_manifests,
        compare_duplicate_pair, detect_entity_outliers, render_phase1e_html_section,
    },
    inspect_review::{
        InspectReviewReport, InspectReviewSource, build_review_report_from_db,
        duplicate_review_score, render_review_html,
    },
    project::{ProjectManifest, SourceFormat, SourceRecord, SourceStatus, WorkspaceLayout},
    publish_skeleton::{
        build_publish_skeleton, render_publish_viewer_html, render_publish_viewer_html_with_data,
    },
};
use std::{collections::BTreeMap, fs, path::PathBuf};

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
        display_name: "DJB-M-SU-監測.dgn.i".to_string(),
        original_file_name: "DJB-M-SU-監測.dgn.i.dgn".to_string(),
        relative_path: PathBuf::from(r"sources\DJB-M-SU-監測.dgn.i.dgn"),
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
        inspect_status: None,
        selected_scale: None,
        warnings: vec![],
    };

    assert_eq!(source.format, SourceFormat::Dgn);
    assert_eq!(source.status, SourceStatus::PendingInspect);
    assert_eq!(source.display_name, "DJB-M-SU-監測.dgn.i");
    assert_eq!(source.original_file_name, "DJB-M-SU-監測.dgn.i.dgn");
}

#[test]
fn project_manifest_serializes_to_stable_json() {
    let manifest = ProjectManifest {
        project_id: "tamkang_bridge".to_string(),
        source_epsg: 3826,
        anchor_source_id: None,
        allowed_scales: vec![1000.0, 1.0, 0.1, 0.01, 0.001],
        sources: vec![SourceRecord {
            id: "dgn-djb-m-su-monitor".to_string(),
            display_name: "DJB-M-SU-監測.dgn.i".to_string(),
            original_file_name: "DJB-M-SU-監測.dgn.i.dgn".to_string(),
            relative_path: PathBuf::from(r"sources\DJB-M-SU-監測.dgn.i.dgn"),
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
            inspect_status: None,
            selected_scale: None,
            warnings: vec![],
        }],
    };
    let json = serde_json::to_string_pretty(&manifest).expect("serialize manifest");

    assert!(json.contains("\"source_epsg\": 3826"));
    assert!(json.contains("\"allowed_scales\""));
    assert!(json.contains("1000.0"));
    assert!(json.contains("0.001"));
    assert!(json.contains("\"display_name\": \"DJB-M-SU-監測.dgn.i\""));
    assert!(json.contains("\"original_file_name\": \"DJB-M-SU-監測.dgn.i.dgn\""));
    assert!(json.contains("\"relative_path\""));
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
fn discover_sources_preserves_human_readable_identity() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let nested = tmp.path().join("cad").join("stage");
    fs::create_dir_all(&nested).expect("nested dir");
    fs::write(nested.join("主橋.dwg"), "dwg").expect("dwg");

    let sources = discover_sources(tmp.path()).expect("discover sources");
    let source = sources
        .iter()
        .find(|source| source.original_file_name == "主橋.dwg")
        .expect("source");

    assert_eq!(source.display_name, "主橋");
    assert_eq!(source.original_file_name, "主橋.dwg");
    assert_eq!(
        source.relative_path,
        PathBuf::from("cad").join("stage").join("主橋.dwg")
    );
}

#[test]
fn discover_sources_flags_repeated_cad_like_extensions() {
    let tmp = tempfile::tempdir().expect("tempdir");
    fs::write(tmp.path().join("DJB-M-SU-監測.dgn.i.dgn"), "dgn").expect("dgn");

    let sources = discover_sources(tmp.path()).expect("discover sources");
    let source = sources
        .iter()
        .find(|source| source.original_file_name == "DJB-M-SU-監測.dgn.i.dgn")
        .expect("source");

    assert!(
        source
            .warnings
            .iter()
            .any(|warning| warning.contains("possible_intermediate_or_export_copy"))
    );
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
fn cad_probe_summary_marks_old_and_preferred_oda_converter() {
    let json = r#"{
      "tools": {
        "ogrinfo": {"found": true, "source": "C:\\ms4w_MSSQL\\GDAL\\ogrinfo.exe", "version": null},
        "ogr2ogr": {"found": true, "source": "C:\\ms4w_MSSQL\\GDAL\\ogr2ogr.exe", "version": null},
        "oda_file_converter": {
          "found": true,
          "source": "C:\\Program Files\\ODA\\ODAFileConverter 27.1.0\\ODAFileConverter.exe",
          "version": "27.1.0.0",
          "version_risk": "acceptable_baseline"
        }
      },
      "oda_file_converters": [
        {
          "found": true,
          "name": "ODAFileConverter.exe",
          "source": "C:\\Program Files\\ODA\\ODAFileConverter 27.1.0\\ODAFileConverter.exe",
          "version": "27.1.0.0",
          "version_major": 27,
          "version_minor": 1,
          "version_build": 0,
          "version_revision": 0,
          "version_risk": "acceptable_baseline",
          "last_write_time": "2026-02-11T15:30:14.0000000+08:00"
        },
        {
          "found": true,
          "name": "ODAFileConverter.exe",
          "source": "C:\\bin\\ODAFileConverter\\ODAFileConverter.exe",
          "version": "20.12.0.0",
          "version_major": 20,
          "version_minor": 12,
          "version_build": 0,
          "version_revision": 0,
          "version_risk": "too_old_for_2026_cad_delivery",
          "last_write_time": "2023-04-23T11:56:10.0000000+08:00"
        }
      ],
      "preferred_oda_file_converter": {
        "found": true,
        "name": "ODAFileConverter.exe",
        "source": "C:\\Program Files\\ODA\\ODAFileConverter 27.1.0\\ODAFileConverter.exe",
        "version": "27.1.0.0",
        "version_major": 27,
        "version_minor": 1,
        "version_build": 0,
        "version_revision": 0,
        "version_risk": "acceptable_baseline",
        "last_write_time": "2026-02-11T15:30:14.0000000+08:00"
      },
      "file_count": 8,
      "cad_file_count": 7,
      "extension_distribution": [
        {"extension": ".dwg", "count": 4, "total_bytes": 149951738},
        {"extension": ".dgn", "count": 3, "total_bytes": 240211456},
        {"extension": ".ifc", "count": 1, "total_bytes": 117439099}
      ],
      "cad_files": []
    }"#;

    let summary: CadProbeSummary = serde_json::from_str(json).expect("parse probe summary");

    assert_eq!(summary.cad_file_count, 7);
    assert_eq!(
        summary.preferred_oda_file_converter.version.as_deref(),
        Some("27.1.0.0")
    );
    assert_eq!(
        summary.tools.oda_file_converter.version_risk.as_deref(),
        Some("acceptable_baseline")
    );
    assert!(
        summary
            .oda_file_converters
            .iter()
            .any(|tool| tool.version_risk.as_deref() == Some("too_old_for_2026_cad_delivery"))
    );
}

#[test]
fn read_cad_probe_summary_loads_probe_report_from_disk() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("cad_probe_report.json");
    fs::write(
        &path,
        r#"{
          "tools": {
            "ogrinfo": {"found": false, "source": null, "version": null},
            "ogr2ogr": {"found": false, "source": null, "version": null},
            "oda_file_converter": {"found": false, "source": null, "version": null, "version_risk": "missing"}
          },
          "oda_file_converters": [],
          "preferred_oda_file_converter": {
            "found": false,
            "name": "ODAFileConverter.exe",
            "source": null,
            "version": null,
            "version_major": 0,
            "version_minor": 0,
            "version_build": 0,
            "version_revision": 0,
            "version_risk": "missing",
            "last_write_time": null
          },
          "file_count": 0,
          "cad_file_count": 0,
          "extension_distribution": [],
          "cad_files": []
        }"#,
    )
    .expect("write probe report");

    let summary = read_cad_probe_summary(&path).expect("read probe report");

    assert_eq!(summary.file_count, 0);
    assert_eq!(
        summary.preferred_oda_file_converter.version_risk.as_deref(),
        Some("missing")
    );
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

#[test]
fn cad_conversion_report_entry_serializes_output_contract() {
    let entry = CadConversionReportEntry {
        source_id: "djb-m-su-dwg-a1b2c3d4".to_string(),
        source_display_name: "DJB-M-SU-監測".to_string(),
        source_original_file_name: "DJB-M-SU-監測.dwg".to_string(),
        source_relative_path: PathBuf::from("DJB-M-SU-監測.dwg"),
        input_path: PathBuf::from(
            r"C:\Users\stw_s\Desktop\ifc_to_3dtiles\sample_files\淡江大橋移交模型\DJB-M-SU-監測.dwg",
        ),
        input_format: SourceFormat::Dwg,
        converted_path: Some(PathBuf::from(
            r"C:\Users\stw_s\Desktop\ifc_to_3dtiles\out\oda_normalized\djb-m-su-dwg-a1b2c3d4\DJB-M-SU-監測_R2018.dwg",
        )),
        converted_format: Some("dwg".to_string()),
        oda_version: Some("27.1.0.0".to_string()),
        target_version: "ACAD2018".to_string(),
        target_format: "DWG".to_string(),
        success: true,
        status: CadConversionStatus::Success,
        input_sha256: "inputhex".to_string(),
        converted_sha256: Some("convertedhex".to_string()),
        bbox_before: None,
        bbox_after: Some(serde_json::json!({
            "raw": null,
            "percentile": null
        })),
        level_count_after: None,
        material_count_after: None,
        fingerprint_after: None,
        warnings: vec![],
        command: None,
        exit_code: None,
    };

    let json = serde_json::to_value(&entry).expect("serialize conversion entry");
    assert_eq!(json["source_id"], "djb-m-su-dwg-a1b2c3d4");
    assert_eq!(json["source_display_name"], "DJB-M-SU-監測");
    assert_eq!(json["source_original_file_name"], "DJB-M-SU-監測.dwg");
    assert_eq!(json["source_relative_path"], "DJB-M-SU-監測.dwg");
    assert_eq!(json["input_format"], "dwg");
    assert_eq!(json["converted_format"], "dwg");
    assert_eq!(json["oda_version"], "27.1.0.0");
    assert_eq!(json["target_version"], "ACAD2018");
    assert_eq!(json["target_format"], "DWG");
    assert_eq!(json["success"], true);
    assert_eq!(json["status"], "success");
    assert!(json["bbox_before"].is_null());
    assert!(json["bbox_after"]["raw"].is_null());

    let parsed: CadConversionReportEntry =
        serde_json::from_value(json).expect("deserialize conversion entry");
    assert_eq!(parsed.status, CadConversionStatus::Success);
    assert_eq!(parsed.converted_sha256.as_deref(), Some("convertedhex"));
}

#[test]
fn normalized_cad_inspect_report_entry_serializes_bbox_contract() {
    let entry = NormalizedCadInspectReportEntry {
        source_id: "djb-m-su-dwg-0c82de78".to_string(),
        source_original_file_name: "DJB-M-SU-監測.dwg".to_string(),
        converted_path: PathBuf::from(
            r"C:\Users\stw_s\Desktop\ifc_to_3dtiles\out\oda_normalized\djb-m-su-dwg-0c82de78\DJB-M-SU-監測.dwg",
        ),
        inspect_success: true,
        ogrinfo_path: Some(PathBuf::from(r"C:\ms4w_MSSQL\GDAL\ogrinfo.exe")),
        exit_code: Some(0),
        bbox_before: None,
        bbox_after: Some(serde_json::json!({
            "raw": [300000.0, 2787000.0, 0.0, 301000.0, 2788000.0, 80.0],
            "percentile": null
        })),
        scale_candidates_after: vec![1.0],
        level_count_after: Some(12),
        material_count_after: None,
        warnings: vec![],
        command: Some(vec![
            "ogrinfo".to_string(),
            "-so".to_string(),
            "DJB-M-SU-監測.dwg".to_string(),
        ]),
    };

    let json = serde_json::to_value(&entry).expect("serialize normalized inspect entry");
    assert_eq!(json["source_id"], "djb-m-su-dwg-0c82de78");
    assert_eq!(json["inspect_success"], true);
    assert_eq!(json["bbox_after"]["raw"][0], 300000.0);
    assert_eq!(json["scale_candidates_after"][0], 1.0);
    assert_eq!(json["level_count_after"], 12);

    let parsed: NormalizedCadInspectReportEntry =
        serde_json::from_value(json).expect("deserialize normalized inspect entry");
    assert!(parsed.inspect_success);
    assert_eq!(parsed.scale_candidates_after, vec![1.0]);
}

#[test]
fn dxf_wkt_scanner_parses_polyhedral_surface_z() {
    let scan = scan_wkt_coordinates(
        "POLYHEDRALSURFACE Z (((292109.5 2785256.5 -0.25,292109.2 2785256.6 0.25)))",
    )
    .expect("scan WKT coordinates");

    assert_eq!(scan.geometry_type, "POLYHEDRALSURFACE");
    assert!(scan.has_z);
    assert_eq!(scan.points.len(), 2);
    assert_eq!(
        scan.bbox,
        [292109.2, 2785256.5, -0.25, 292109.5, 2785256.6, 0.25]
    );
}

#[test]
fn ogrinfo_entity_parser_extracts_feature_metadata_and_bbox() {
    let text = r#"
OGRFeature(entities):0
  FID (Integer) = 0
  Layer (String) = 靜態應變計
  SubClasses (String) = AcDbEntity:AcDbBlockReference
  Linetype (String) = ByLayer
  EntityHandle (String) = 14C5
  Style = PEN(c:#000000,w:2.1g)
  POLYHEDRALSURFACE Z (((292109.5 2785256.5 -0.25,292109.2 2785256.6 0.25)))
"#;

    let entities = parse_ogrinfo_entities("bridge-dwg", text);

    assert_eq!(entities.len(), 1);
    assert_eq!(entities[0].source_id, "bridge-dwg");
    assert_eq!(entities[0].fid, 0);
    assert_eq!(entities[0].layer, "靜態應變計");
    assert_eq!(
        entities[0].subclasses.as_deref(),
        Some("AcDbEntity:AcDbBlockReference")
    );
    assert_eq!(entities[0].entity_handle.as_deref(), Some("14C5"));
    assert_eq!(
        entities[0].geometry_type.as_deref(),
        Some("POLYHEDRALSURFACE")
    );
    assert_eq!(
        entities[0].bbox,
        Some([292109.2, 2785256.5, -0.25, 292109.5, 2785256.6, 0.25])
    );
}

#[test]
fn entity_stats_use_percentile_bbox_for_scale_and_ignore_outlier() {
    let mut entities = Vec::new();
    for idx in 0..1000 {
        let x = 292000.0 + (idx % 20) as f64;
        let y = 2785000.0 + (idx / 20) as f64;
        entities.push(CadEntity::with_bbox(
            "bridge-dwg",
            idx,
            "Cable",
            "LINESTRING",
            [x, y, 1.0, x + 1.0, y + 1.0, 5.0],
            2,
            true,
        ));
    }
    entities.push(CadEntity::with_bbox(
        "bridge-dwg",
        9999,
        "Garbage",
        "LINESTRING",
        [-2_344_516.0, 0.0, 0.0, -2_344_500.0, 10.0, 0.0],
        2,
        false,
    ));

    let stats = summarize_entities("bridge-dwg", &entities, &[1000.0, 1.0, 0.1, 0.01, 0.001]);

    assert_eq!(stats.entity_count, 1001);
    assert_eq!(stats.parsed_entity_count, 1001);
    assert_eq!(stats.selected_scale, Some(1.0));
    assert_eq!(stats.inspect_status, "approved");
    assert!(stats.raw_bbox[0] < 0.0);
    assert!(stats.percentile_bbox[0] > 291000.0);
    assert_eq!(stats.layer_histogram.get("Cable"), Some(&1000));
}

#[test]
fn dxf_entity_fingerprint_is_stable_for_same_stats() {
    let stats = CadEntityStats {
        source_id: "bridge-dwg".to_string(),
        entity_count: 10,
        parsed_entity_count: 9,
        skipped_entity_count: 1,
        vertex_count: 200,
        raw_bbox: [0.0, 0.0, 0.0, 10.0, 10.0, 2.0],
        percentile_bbox: [1.0, 1.0, 0.0, 9.0, 9.0, 2.0],
        z_range: 2.0,
        selected_scale: Some(1.0),
        inspect_status: "approved".to_string(),
        layer_histogram: std::collections::BTreeMap::from([("Cable".to_string(), 9)]),
        geometry_type_histogram: std::collections::BTreeMap::from([(
            "POLYHEDRALSURFACE".to_string(),
            9,
        )]),
        fingerprint_hash: String::new(),
        warnings: vec![],
    };

    let first = entity_fingerprint_hash(&stats);
    let second = entity_fingerprint_hash(&stats);

    assert_eq!(first, second);
    assert!(first.len() >= 16);
}

#[test]
fn entity_inspect_sqlite_schema_accepts_sources_entities_and_stats() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let db_path = tmp.path().join("project_inspect.db");
    let entity = CadEntity::with_bbox(
        "bridge-dwg",
        1,
        "Cable",
        "POLYHEDRALSURFACE",
        [292000.0, 2785000.0, 0.0, 292001.0, 2785001.0, 2.0],
        8,
        true,
    );
    let stats = summarize_entities("bridge-dwg", &[entity.clone()], &[1.0]);

    write_entity_inspect_db(&db_path, &[entity], &[stats]).expect("write entity inspect db");

    let conn = rusqlite::Connection::open(&db_path).expect("open sqlite");
    let entity_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM entities", [], |row| row.get(0))
        .expect("count entities");
    let stats_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM source_stats", [], |row| row.get(0))
        .expect("count stats");

    assert_eq!(entity_count, 1);
    assert_eq!(stats_count, 1);
}

#[test]
fn inspect_review_scores_probable_duplicate_dwg_sources() {
    let first = review_stats(
        "djb-m-su-dwg-0c82de78",
        [292000.0, 2785000.0, -40.0, 293000.0, 2786000.0, 170.0],
        24_640,
        2_696_555,
        [
            ("_CIVIL_CONSTRUCTION", 18_439),
            ("Cable", 755),
            ("STEEL", 576),
        ],
    );
    let second = review_stats(
        "dwg-850173d8",
        [292001.0, 2785001.0, -40.0, 293001.0, 2786001.0, 170.2],
        24_077,
        2_677_659,
        [
            ("_CIVIL_CONSTRUCTION", 18_439),
            ("Cable", 755),
            ("STEEL", 576),
        ],
    );

    let score = duplicate_review_score(&first, &second);

    assert!(score >= 0.9, "score was {score}");
}

#[test]
fn inspect_review_explains_quarantine_without_calling_it_2d_when_z_range_is_real() {
    let source = InspectReviewSource::from_stats(
        "djb-m-su-dwg-0c82de78",
        "DJB-M-SU-監測.dwg",
        "dwg",
        "quarantined",
        &review_stats(
            "djb-m-su-dwg-0c82de78",
            [-2344516.9, 2784696.2, -40.2, 292398.9, 3730959.0, 169.78],
            24_640,
            2_696_555,
            [
                ("_CIVIL_CONSTRUCTION", 18_439),
                ("Cable", 755),
                ("STEEL", 576),
            ],
        ),
        vec!["source bounds outside AOI for all allowed scales".to_string()],
    );

    assert!(
        source
            .quarantine_reasons
            .iter()
            .any(|reason| reason.contains("超出 AOI"))
    );
    assert!(
        source
            .quarantine_reasons
            .iter()
            .any(|reason| reason.contains("不是 2D"))
    );
    assert!(
        source
            .quarantine_reasons
            .iter()
            .all(|reason| !reason.contains("可能是 2D"))
    );
}

#[test]
fn inspect_review_html_contains_source_status_bbox_warnings_and_duplicate_score() {
    let mut bridge_a = InspectReviewSource::from_stats(
        "djb-m-su-dwg-0c82de78",
        "DJB-M-SU-監測.dwg",
        "dwg",
        "quarantined",
        &review_stats(
            "djb-m-su-dwg-0c82de78",
            [292000.0, 2785000.0, -40.0, 293000.0, 2786000.0, 170.0],
            24_640,
            2_696_555,
            [
                ("_CIVIL_CONSTRUCTION", 18_439),
                ("Cable", 755),
                ("STEEL", 576),
            ],
        ),
        vec!["source bounds outside AOI for all allowed scales".to_string()],
    );
    bridge_a.add_duplicate_candidate("dwg-850173d8", "主橋.dwg", 0.96);
    let report = InspectReviewReport {
        project_id: "淡江大橋移交模型".to_string(),
        generated_at: "test".to_string(),
        source_count: 8,
        sources: vec![bridge_a],
    };

    let html = render_review_html(&report);

    assert!(html.contains("Phase 1D Inspect Review"));
    assert!(html.contains("DJB-M-SU-監測.dwg"));
    assert!(html.contains("quarantined"));
    assert!(html.contains("P0.5/P99.5"));
    assert!(html.contains("source bounds outside AOI"));
    assert!(html.contains("主橋.dwg"));
    assert!(html.contains("96.0%"));
}

#[test]
fn inspect_review_can_build_report_from_sqlite_and_manifest() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let db_path = tmp.path().join("project_inspect.db");
    let entity = CadEntity::with_bbox(
        "dwg-12d5f1b6",
        1,
        "BridgeTower",
        "POLYHEDRALSURFACE",
        [292000.0, 2785000.0, 0.0, 292001.0, 2785001.0, 30.0],
        8,
        true,
    );
    let stats = summarize_entities("dwg-12d5f1b6", &[entity.clone()], &[1.0]);
    write_entity_inspect_db(&db_path, &[entity], &[stats]).expect("write db");
    let manifest_path = tmp.path().join("source_manifest.json");
    fs::write(
        &manifest_path,
        r#"{
          "project_id": "淡江大橋移交模型",
          "source_epsg": 3826,
          "anchor_source_id": null,
          "allowed_scales": [1000.0, 1.0, 0.1, 0.01, 0.001],
          "sources": [
            {
              "id": "dwg-12d5f1b6",
              "display_name": "主橋塔",
              "original_file_name": "主橋塔.dwg",
              "relative_path": "主橋塔.dwg",
              "path": "C:\\sample\\主橋塔.dwg",
              "format": "dwg",
              "status": "approved",
              "original_size_bytes": 25339290,
              "detected_crs": null,
              "unit_scale_to_meter": 1.0,
              "anchor_distance_m": null,
              "raw_bbox": null,
              "percentile_bbox": null,
              "transform": null,
              "cad_metadata_path": null,
              "fingerprint_hash": null,
              "duplicate_candidates": [],
              "inspect_status": "approved",
              "selected_scale": 1.0,
              "warnings": []
            }
          ]
        }"#,
    )
    .expect("write manifest");

    let report = build_review_report_from_db(&db_path, &manifest_path).expect("build report");

    assert_eq!(report.project_id, "淡江大橋移交模型");
    assert_eq!(report.source_count, 1);
    assert_eq!(report.sources[0].original_file_name, "主橋塔.dwg");
    assert_eq!(report.sources[0].inspect_status, "approved");
    assert_eq!(report.sources[0].selected_scale, Some(1.0));
}

#[test]
fn phase1e_duplicate_compare_recommends_monitoring_source_over_main_bridge_duplicate() {
    let monitor = review_stats(
        "djb-m-su-dwg-0c82de78",
        [-2344516.9, 2784696.2, -40.2, 292398.9, 3730959.0, 169.78],
        24_640,
        2_696_555,
        [
            ("_CIVIL_CONSTRUCTION", 18_439),
            ("Cable", 755),
            ("STEEL", 576),
        ],
    );
    let main_bridge = review_stats(
        "dwg-850173d8",
        [-2344516.9, 2784696.2, -16.4, 292398.9, 3730959.0, 166.39],
        24_077,
        2_677_659,
        [
            ("_CIVIL_CONSTRUCTION", 18_439),
            ("Cable", 755),
            ("STEEL", 576),
        ],
    );

    let pair = compare_duplicate_pair("DJB-M-SU-監測.dwg", &monitor, "主橋.dwg", &main_bridge);

    assert!(pair.score > 0.9);
    assert_eq!(pair.retain_source_id, "djb-m-su-dwg-0c82de78");
    assert_eq!(pair.reject_source_id, "dwg-850173d8");
    assert!(pair.recommendation_reason.contains("完整監測交付主檔"));
    assert!(pair.layer_count_diff.contains_key("_CIVIL_CONSTRUCTION"));
}

#[test]
fn phase1e_outlier_detector_finds_far_entity_largest_bbox_and_z_range() {
    let records = vec![
        EntityBboxRecord::new(
            "dwg-dd37eec7",
            1,
            "normal",
            Some("A1".to_string()),
            Some("POLYHEDRALSURFACE".to_string()),
            20,
            [292000.0, 2785000.0, 1.0, 292010.0, 2785010.0, 10.0],
        ),
        EntityBboxRecord::new(
            "dwg-dd37eec7",
            2,
            "far_layer",
            Some("FAR".to_string()),
            Some("POLYHEDRALSURFACE".to_string()),
            20,
            [
                292365000.0,
                2785739000.0,
                0.0,
                292365795.0,
                2785739723.0,
                10.0,
            ],
        ),
        EntityBboxRecord::new(
            "dwg-dd37eec7",
            3,
            "tower_z",
            Some("Z1".to_string()),
            Some("LINESTRING".to_string()),
            2,
            [292050.0, 2785050.0, 0.0, 292055.0, 2785055.0, 18109.0],
        ),
    ];

    let report = detect_entity_outliers("dwg-dd37eec7", "管理中心_全.dwg", &records, 5);

    assert!(
        report
            .outliers
            .iter()
            .any(|outlier| outlier.fid == 2 && outlier.reason == "far_from_source_center")
    );
    assert!(
        report
            .outliers
            .iter()
            .any(|outlier| outlier.fid == 2 && outlier.reason == "largest_bbox_diagonal")
    );
    assert!(
        report
            .outliers
            .iter()
            .any(|outlier| outlier.fid == 3 && outlier.reason == "largest_z_range")
    );
    assert!(
        report
            .layer_outliers
            .iter()
            .any(|layer| layer.layer == "far_layer" && layer.entity_count == 1)
    );
}

#[test]
fn phase1e_approval_classifier_splits_approved_rejected_and_needs_review_sources() {
    let mut approved = InspectReviewSource::from_stats(
        "dwg-12d5f1b6",
        "主橋塔.dwg",
        "dwg",
        "approved",
        &review_stats(
            "dwg-12d5f1b6",
            [292106.8, 2785254.6, -4.8, 292180.2, 2785518.2, 186.0],
            1_314,
            692_642,
            [("電梯軌道", 854)],
        ),
        vec![],
    );
    approved.selected_scale = Some(1.0);
    let monitor = InspectReviewSource::from_stats(
        "djb-m-su-dwg-0c82de78",
        "DJB-M-SU-監測.dwg",
        "dwg",
        "quarantined",
        &review_stats(
            "djb-m-su-dwg-0c82de78",
            [-2344516.9, 2784696.2, -40.2, 292398.9, 3730959.0, 169.78],
            24_640,
            2_696_555,
            [("_CIVIL_CONSTRUCTION", 18_439)],
        ),
        vec![],
    );
    let main_bridge = InspectReviewSource::from_stats(
        "dwg-850173d8",
        "主橋.dwg",
        "dwg",
        "quarantined",
        &review_stats(
            "dwg-850173d8",
            [-2344516.9, 2784696.2, -16.4, 292398.9, 3730959.0, 166.39],
            24_077,
            2_677_659,
            [("_CIVIL_CONSTRUCTION", 18_439)],
        ),
        vec![],
    );
    let dgn = InspectReviewSource {
        source_id: "dgn-i-dgn-cd887b3a".to_string(),
        display_name: "管理中心_全".to_string(),
        original_file_name: "管理中心_全.dgn.i.dgn".to_string(),
        format: "dgn".to_string(),
        inspect_status: "needs_alternative_route".to_string(),
        selected_scale: None,
        entity_count: 0,
        parsed_entity_count: 0,
        skipped_entity_count: 0,
        vertex_count: 0,
        raw_bbox: None,
        percentile_bbox: None,
        z_range: None,
        fingerprint_hash: None,
        layer_histogram: Default::default(),
        geometry_type_histogram: Default::default(),
        warnings: vec!["DGN needs alternative route: ODA invalid group code".to_string()],
        quarantine_reasons: vec![],
        duplicate_candidates: vec![],
    };
    let duplicate = compare_duplicate_pair(
        "DJB-M-SU-監測.dwg",
        &monitor_stats(),
        "主橋.dwg",
        &main_bridge_stats(),
    );

    let manifests =
        classify_approval_manifests(&[approved, monitor, main_bridge, dgn], &[duplicate]);

    assert_source(&manifests.approved.sources, "dwg-12d5f1b6");
    assert_source(&manifests.rejected.sources, "dwg-850173d8");
    assert_source(&manifests.needs_review.sources, "djb-m-su-dwg-0c82de78");
    assert_source(&manifests.needs_review.sources, "dgn-i-dgn-cd887b3a");
}

#[test]
fn phase1e_html_section_contains_duplicate_outlier_and_manifest_summary() {
    let duplicate = compare_duplicate_pair(
        "DJB-M-SU-監測.dwg",
        &monitor_stats(),
        "主橋.dwg",
        &main_bridge_stats(),
    );
    let outliers = detect_entity_outliers(
        "dwg-dd37eec7",
        "管理中心_全.dwg",
        &[EntityBboxRecord::new(
            "dwg-dd37eec7",
            9,
            "S-BEAM-CONC",
            Some("H9".to_string()),
            Some("POLYHEDRALSURFACE".to_string()),
            100,
            [0.0, 0.0, 0.0, 292365795.0, 2785739723.0, 18109.0],
        )],
        3,
    );
    let manifests = classify_approval_manifests(&[], &[duplicate.clone()]);

    let html = render_phase1e_html_section(&[duplicate], Some(&outliers), &manifests);

    assert!(html.contains("Phase 1E QA"));
    assert!(html.contains("DJB-M-SU-監測.dwg"));
    assert!(html.contains("主橋.dwg"));
    assert!(html.contains("管理中心_全.dwg"));
    assert!(html.contains("S-BEAM-CONC"));
    assert!(html.contains("approved_sources.json"));
}

#[test]
fn phase1f_publish_skeleton_only_publishes_approved_sources() {
    let manifest = phase1f_project_manifest();
    let approvals = phase1f_approval_manifests();
    let converted_paths = BTreeMap::from([
        (
            "dwg-12d5f1b6".to_string(),
            PathBuf::from(r"C:\normalized\dwg-12d5f1b6\主橋塔.dxf"),
        ),
        (
            "dwg-850173d8".to_string(),
            PathBuf::from(r"C:\normalized\dwg-850173d8\主橋.dxf"),
        ),
        (
            "dwg-dd37eec7".to_string(),
            PathBuf::from(r"C:\normalized\dwg-dd37eec7\管理中心_全.dxf"),
        ),
    ]);

    let skeleton = build_publish_skeleton(&manifest, &approvals, &converted_paths);

    assert_eq!(skeleton.sources_manifest.sources.len(), 1);
    assert_eq!(
        skeleton.sources_manifest.sources[0].source_id,
        "dwg-12d5f1b6"
    );
    assert!(
        skeleton
            .sources_manifest
            .sources
            .iter()
            .all(|source| source.approval_decision == "approved")
    );
    assert!(
        skeleton
            .sources_manifest
            .sources
            .iter()
            .all(|source| source.source_id != "dwg-850173d8")
    );
    assert!(
        skeleton
            .sources_manifest
            .sources
            .iter()
            .all(|source| source.source_id != "dwg-dd37eec7")
    );
}

#[test]
fn phase1f_debug_overlay_keeps_rejected_and_needs_review_metadata_out_of_publish() {
    let manifest = phase1f_project_manifest();
    let approvals = phase1f_approval_manifests();
    let skeleton = build_publish_skeleton(&manifest, &approvals, &BTreeMap::new());

    let rejected = skeleton
        .debug_overlays
        .sources
        .iter()
        .find(|source| source.source_id == "dwg-850173d8")
        .expect("rejected overlay");
    assert_eq!(rejected.approval_decision, "rejected");
    assert_eq!(
        rejected.duplicate_of.as_deref(),
        Some("djb-m-su-dwg-0c82de78")
    );
    assert!(rejected.bbox.is_some());

    let dgn = skeleton
        .debug_overlays
        .sources
        .iter()
        .find(|source| source.source_id == "dgn-i-dgn-cd887b3a")
        .expect("DGN overlay metadata");
    assert_eq!(dgn.approval_decision, "needs_review");
    assert!(dgn.bbox.is_none());
    assert!(
        dgn.warnings
            .iter()
            .any(|warning| warning.contains("no bbox available"))
    );
}

#[test]
fn phase1f_normalized_manifest_preserves_source_id_and_traceability_without_copying_cad() {
    let manifest = phase1f_project_manifest();
    let approvals = phase1f_approval_manifests();
    let converted_paths = BTreeMap::from([(
        "dwg-12d5f1b6".to_string(),
        PathBuf::from(r"C:\normalized\dwg-12d5f1b6\主橋塔.dxf"),
    )]);

    let skeleton = build_publish_skeleton(&manifest, &approvals, &converted_paths);
    let normalized = skeleton
        .normalized_sources
        .iter()
        .find(|source| source.source_id == "dwg-12d5f1b6")
        .expect("normalized approved source");
    let json = serde_json::to_value(normalized).expect("serialize normalized manifest");

    assert_eq!(json["source_id"], "dwg-12d5f1b6");
    assert_eq!(json["approval_decision"], "approved");
    assert_eq!(
        json["converted_path"],
        r"C:\normalized\dwg-12d5f1b6\主橋塔.dxf"
    );
    assert!(json.get("copy_cad_file").is_none());
}

#[test]
fn phase1f_publish_viewer_html_has_three_bbox_toggles_and_metadata_fields() {
    let html = render_publish_viewer_html();

    assert!(html.contains("approved only"));
    assert!(html.contains("rejected bbox"));
    assert!(html.contains("needs review bbox"));
    assert!(html.contains("approvedToggle"));
    assert!(html.contains("rejectedToggle"));
    assert!(html.contains("needsReviewToggle"));
    assert!(html.contains("sources_manifest.json"));
    assert!(html.contains("debug_overlays.json"));
    assert!(html.contains("source_id"));
    assert!(html.contains("approval_decision"));
    assert!(html.contains("duplicate_of"));
    assert!(html.contains("Cesium-1.141 missing"));
    assert!(!html.contains("const outlines"));
}

#[test]
fn phase1f_publish_viewer_uses_embedded_manifests_for_file_mode() {
    let manifest = phase1f_project_manifest();
    let approvals = phase1f_approval_manifests();
    let skeleton = build_publish_skeleton(&manifest, &approvals, &BTreeMap::new());

    let html = render_publish_viewer_html_with_data(Some(&skeleton));

    assert!(html.contains("embeddedSourcesManifest"));
    assert!(html.contains("embeddedDebugOverlays"));
    assert!(html.contains("readEmbeddedJson"));
    assert!(html.contains("location.protocol === \"file:\""));
    assert!(html.contains("dwg-12d5f1b6"));
    assert!(html.contains("dwg-850173d8"));
}

#[test]
fn phase1f_publish_viewer_disables_default_network_imagery() {
    let html = render_publish_viewer_html();

    assert!(html.contains("baseLayer: false"));
    assert!(html.contains("imageryProvider: false"));
    assert!(html.contains("baseLayerPicker: false"));
    assert!(html.contains("showRenderLoopErrors: false"));
    assert!(html.contains("scene.renderError.addEventListener"));
    assert!(!html.contains("baseLayerPicker: true"));
}

#[test]
fn phase1f_publish_viewer_adds_emap5_wmts_context_layer() {
    let html = render_publish_viewer_html();

    assert!(html.contains("createEmap5WmtsProvider"));
    assert!(html.contains("wmts.nlsc.gov.tw/wmts/EMAP5/default/GoogleMapsCompatible/{z}/{y}/{x}"));
    assert!(html.contains("viewer.imageryLayers.addImageryProvider"));
    assert!(html.contains("EMAP5 WMTS"));
    assert!(html.contains("provider.errorEvent.addEventListener"));
}

fn review_stats<const N: usize>(
    source_id: &str,
    percentile_bbox: [f64; 6],
    entity_count: u64,
    vertex_count: u64,
    layers: [(&str, u64); N],
) -> CadEntityStats {
    CadEntityStats {
        source_id: source_id.to_string(),
        entity_count,
        parsed_entity_count: entity_count,
        skipped_entity_count: 0,
        vertex_count,
        raw_bbox: percentile_bbox,
        percentile_bbox,
        z_range: (percentile_bbox[5] - percentile_bbox[2]).abs(),
        selected_scale: None,
        inspect_status: "quarantined".to_string(),
        layer_histogram: layers
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect(),
        geometry_type_histogram: std::collections::BTreeMap::from([(
            "POLYHEDRALSURFACE".to_string(),
            entity_count,
        )]),
        fingerprint_hash: "hash".to_string(),
        warnings: vec![],
    }
}

fn monitor_stats() -> CadEntityStats {
    review_stats(
        "djb-m-su-dwg-0c82de78",
        [-2344516.9, 2784696.2, -40.2, 292398.9, 3730959.0, 169.78],
        24_640,
        2_696_555,
        [
            ("_CIVIL_CONSTRUCTION", 18_439),
            ("Cable", 755),
            ("STEEL", 576),
        ],
    )
}

fn main_bridge_stats() -> CadEntityStats {
    review_stats(
        "dwg-850173d8",
        [-2344516.9, 2784696.2, -16.4, 292398.9, 3730959.0, 166.39],
        24_077,
        2_677_659,
        [
            ("_CIVIL_CONSTRUCTION", 18_439),
            ("Cable", 755),
            ("STEEL", 576),
        ],
    )
}

fn assert_source(sources: &[ApprovalSourceDecision], source_id: &str) {
    assert!(
        sources.iter().any(|source| source.source_id == source_id),
        "missing source {source_id}"
    );
}

fn phase1f_project_manifest() -> ProjectManifest {
    ProjectManifest {
        project_id: "淡江大橋移交模型".to_string(),
        source_epsg: 3826,
        anchor_source_id: None,
        allowed_scales: vec![1000.0, 1.0, 0.1, 0.01, 0.001],
        sources: vec![
            phase1f_source(
                "dwg-12d5f1b6",
                "主橋塔",
                "主橋塔.dwg",
                SourceFormat::Dwg,
                SourceStatus::Approved,
                Some([292106.8, 2785254.6, -4.8, 292180.2, 2785518.2, 186.0]),
                Some(1.0),
            ),
            phase1f_source(
                "dwg-850173d8",
                "主橋",
                "主橋.dwg",
                SourceFormat::Dwg,
                SourceStatus::Quarantined,
                Some([-2344516.9, 2784696.2, -16.4, 292398.9, 3730959.0, 166.39]),
                None,
            ),
            phase1f_source(
                "dwg-dd37eec7",
                "管理中心_全",
                "管理中心_全.dwg",
                SourceFormat::Dwg,
                SourceStatus::Quarantined,
                Some([0.0, 0.0, 0.0, 292365795.4, 2785739723.5, 18109.8]),
                None,
            ),
            phase1f_source(
                "dgn-i-dgn-cd887b3a",
                "管理中心_全.dgn.i",
                "管理中心_全.dgn.i.dgn",
                SourceFormat::Dgn,
                SourceStatus::NeedsAlternativeRoute,
                None,
                None,
            ),
            phase1f_source(
                "djb-m-su-ifc-21833332",
                "DJB-M-SU-監測",
                "DJB-M-SU-監測.ifc",
                SourceFormat::Ifc,
                SourceStatus::PendingInspect,
                None,
                None,
            ),
        ],
    }
}

fn phase1f_source(
    id: &str,
    display_name: &str,
    original_file_name: &str,
    format: SourceFormat,
    status: SourceStatus,
    bbox: Option<[f64; 6]>,
    selected_scale: Option<f64>,
) -> SourceRecord {
    SourceRecord {
        id: id.to_string(),
        display_name: display_name.to_string(),
        original_file_name: original_file_name.to_string(),
        relative_path: PathBuf::from(original_file_name),
        path: PathBuf::from(r"C:\sample").join(original_file_name),
        format,
        status,
        original_size_bytes: 100,
        detected_crs: None,
        unit_scale_to_meter: selected_scale,
        anchor_distance_m: None,
        raw_bbox: bbox,
        percentile_bbox: bbox,
        transform: None,
        cad_metadata_path: None,
        fingerprint_hash: Some(format!("{id}-hash")),
        duplicate_candidates: vec![],
        inspect_status: Some(
            match status {
                SourceStatus::Approved => "approved",
                SourceStatus::Quarantined => "quarantined",
                SourceStatus::NeedsAlternativeRoute => "needs_alternative_route",
                SourceStatus::PendingInspect => "pending_inspect",
                SourceStatus::Converted => "converted",
                SourceStatus::Published => "published",
            }
            .to_string(),
        ),
        selected_scale,
        warnings: vec![],
    }
}

fn phase1f_approval_manifests() -> ifc_to_3dtiles::inspect_drilldown::ApprovalManifests {
    ifc_to_3dtiles::inspect_drilldown::ApprovalManifests {
        approved: ifc_to_3dtiles::inspect_drilldown::ApprovalManifest {
            generated_at: "test".to_string(),
            decision: "approved".to_string(),
            sources: vec![phase1f_decision(
                "dwg-12d5f1b6",
                "主橋塔.dwg",
                "dwg",
                "approved",
                "approved",
                "entity inspect approved with selected scale",
                None,
            )],
        },
        rejected: ifc_to_3dtiles::inspect_drilldown::ApprovalManifest {
            generated_at: "test".to_string(),
            decision: "rejected".to_string(),
            sources: vec![phase1f_decision(
                "dwg-850173d8",
                "主橋.dwg",
                "dwg",
                "quarantined",
                "rejected",
                "duplicate_candidate",
                Some("djb-m-su-dwg-0c82de78"),
            )],
        },
        needs_review: ifc_to_3dtiles::inspect_drilldown::ApprovalManifest {
            generated_at: "test".to_string(),
            decision: "needs_review".to_string(),
            sources: vec![
                phase1f_decision(
                    "dwg-dd37eec7",
                    "管理中心_全.dwg",
                    "dwg",
                    "quarantined",
                    "needs_review",
                    "requires human QA before publish",
                    None,
                ),
                phase1f_decision(
                    "dgn-i-dgn-cd887b3a",
                    "管理中心_全.dgn.i.dgn",
                    "dgn",
                    "needs_alternative_route",
                    "needs_review",
                    "DGN needs alternative route: ODA invalid group code",
                    None,
                ),
                phase1f_decision(
                    "djb-m-su-ifc-21833332",
                    "DJB-M-SU-監測.ifc",
                    "ifc",
                    "pending_inspect",
                    "needs_review",
                    "requires human QA before publish",
                    None,
                ),
            ],
        },
    }
}

fn phase1f_decision(
    source_id: &str,
    original_file_name: &str,
    format: &str,
    inspect_status: &str,
    decision: &str,
    reason: &str,
    duplicate_of: Option<&str>,
) -> ApprovalSourceDecision {
    ApprovalSourceDecision {
        source_id: source_id.to_string(),
        original_file_name: original_file_name.to_string(),
        format: format.to_string(),
        inspect_status: inspect_status.to_string(),
        decision: decision.to_string(),
        reason: reason.to_string(),
        duplicate_of: duplicate_of.map(str::to_string),
    }
}
