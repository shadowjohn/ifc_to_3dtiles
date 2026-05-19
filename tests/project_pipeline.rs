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
