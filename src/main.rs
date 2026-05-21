use std::{
    fs,
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use clap::{Parser, Subcommand, ValueEnum};
use ifc_to_3dtiles::{
    ConvertOptions, NormalMode,
    cad_conversion::CadConversionReport,
    cad_entity_inspect::{
        CadEntity, EntityInspectReport, parse_ogrinfo_entities, summarize_entities,
        write_entity_inspect_db,
    },
    convert_path,
    geometry_preview::write_geometry_preview_outputs,
    ifc_info::write_ifc_info_path,
    inspect::discover_sources,
    inspect_drilldown::write_drilldown_outputs,
    inspect_review::write_review_report_html,
    project::{ProjectManifest, SourceFormat, SourceStatus},
    publish_skeleton::write_publish_skeleton_outputs,
    revit::RevitVersion,
    runtime_publish::write_runtime_publish_outputs,
    rvt::{RvtToIfcOptions, export_rvt_to_ifc},
};
use rusqlite::params;

const DEFAULT_TILE_MAX_FEATURES: usize = 50;
const DEFAULT_TILE_MAX_TRIANGLES: usize = 20_000;

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliNormalMode {
    Flat,
    Smooth,
    Both,
}

impl From<CliNormalMode> for NormalMode {
    fn from(value: CliNormalMode) -> Self {
        match value {
            CliNormalMode::Flat => Self::Flat,
            CliNormalMode::Smooth => Self::Smooth,
            CliNormalMode::Both => Self::Both,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CliRevitVersion {
    Auto,
    #[value(name = "2025")]
    V2025,
    #[value(name = "2026")]
    V2026,
    #[value(name = "2027")]
    V2027,
}

impl CliRevitVersion {
    fn requested(self) -> Option<RevitVersion> {
        match self {
            Self::Auto => None,
            Self::V2025 => Some(RevitVersion::V2025),
            Self::V2026 => Some(RevitVersion::V2026),
            Self::V2027 => Some(RevitVersion::V2027),
        }
    }
}

#[derive(Debug, Parser)]
#[command(name = "ifc_to_3dtiles")]
#[command(about = "Convert IFC or RVT models to GLB and Cesium 3D Tiles")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    #[arg(long)]
    input: Option<PathBuf>,

    #[arg(long)]
    output: Option<PathBuf>,

    #[arg(
        long,
        default_value_t = 3826,
        help = "Source CRS EPSG code: 3825, 3826, 3827, 3828, 4326, or 3857"
    )]
    source_epsg: u32,

    #[arg(long, default_value_t = DEFAULT_TILE_MAX_FEATURES)]
    tile_max_features: usize,

    #[arg(long, default_value_t = DEFAULT_TILE_MAX_TRIANGLES)]
    tile_max_triangles: usize,

    #[arg(
        long,
        value_enum,
        default_value_t = CliNormalMode::Both,
        help = "Normal output mode: flat, smooth, or both for viewer 平面/平滑切換"
    )]
    normal_mode: CliNormalMode,

    #[arg(
        long,
        default_value_t = 90.0,
        help = "Smooth normal angle threshold, 0..180 degrees"
    )]
    smooth_angle_deg: f64,

    #[arg(long)]
    overwrite: bool,

    #[arg(
        long,
        value_enum,
        default_value_t = CliRevitVersion::Auto,
        help = "RVT only: auto-detect Revit or force 2025, 2026, 2027"
    )]
    revit_version: CliRevitVersion,

    #[arg(
        long,
        help = "RVT only: keep the intermediate IFC beside generated output"
    )]
    keep_ifc: bool,

    #[arg(long, help = "RVT only: path to RvtToGlb.RevitIfcExporter.dll")]
    bridge_assembly: Option<PathBuf>,

    #[arg(
        long,
        help = "RVT only: path to Revit.exe when auto-detect cannot find it"
    )]
    revit_exe: Option<PathBuf>,

    #[arg(long, default_value_t = 30, help = "RVT only: Revit export timeout")]
    rvt_timeout_minutes: u64,
}

#[derive(Debug, Subcommand)]
enum Command {
    Inspect {
        #[arg(long)]
        input: PathBuf,

        #[arg(long)]
        output: PathBuf,

        #[arg(
            long,
            default_value_t = 3826,
            help = "Source CRS EPSG code: 3825, 3826, 3827, 3828, 4326, or 3857"
        )]
        source_epsg: u32,
    },
    EntityInspectDxf {
        #[arg(long)]
        conversion_report: PathBuf,

        #[arg(long)]
        manifest: PathBuf,

        #[arg(long)]
        output: PathBuf,

        #[arg(long, default_value_t = 0)]
        batch_size: usize,

        #[arg(long)]
        ogrinfo: Option<PathBuf>,
    },
    InspectReview {
        #[arg(long)]
        input: PathBuf,

        #[arg(long)]
        output: Option<PathBuf>,

        #[arg(long)]
        db: Option<PathBuf>,

        #[arg(long)]
        manifest: Option<PathBuf>,
    },
    InspectDrilldown {
        #[arg(long)]
        input: PathBuf,

        #[arg(long)]
        output: Option<PathBuf>,

        #[arg(long)]
        db: Option<PathBuf>,

        #[arg(long)]
        manifest: Option<PathBuf>,
    },
    PublishApproved {
        #[arg(long)]
        input: PathBuf,

        #[arg(long)]
        output: Option<PathBuf>,
    },
    RuntimePublish {
        #[arg(long)]
        input: PathBuf,

        #[arg(long)]
        output: Option<PathBuf>,
    },
    #[command(name = "geometry-preview")]
    GeometryPreview {
        #[arg(long)]
        input: PathBuf,

        #[arg(long)]
        output: Option<PathBuf>,
    },
    #[command(name = "ifc-info")]
    IfcInfo {
        #[arg(long)]
        input: PathBuf,

        #[arg(long)]
        output: PathBuf,
    },
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let cli = Cli::parse();
    if let Some(command) = &cli.command {
        return run_command(command);
    }

    let input = cli
        .input
        .clone()
        .context("直接轉檔模式需要 --input；或使用 inspect --input")?;
    let output = cli
        .output
        .clone()
        .context("直接轉檔模式需要 --output；或使用 inspect --output")?;

    ensure!(
        (0.0..=180.0).contains(&cli.smooth_angle_deg),
        "--smooth-angle-deg 必須介於 0 到 180"
    );
    ensure!(
        cli.rvt_timeout_minutes > 0,
        "--rvt-timeout-minutes 必須大於 0"
    );
    let rvt_timeout_secs = cli
        .rvt_timeout_minutes
        .checked_mul(60)
        .context("--rvt-timeout-minutes 太大")?;

    let mut input = input;
    let mut cleanup_after_convert = Vec::new();
    if has_extension(&input, "rvt") {
        fs::create_dir_all(&output)
            .with_context(|| format!("建立輸出目錄失敗：{}", output.display()))?;
        let ifc_path = output.join(format!("{}.ifc", safe_stem(&input)));
        if ifc_path.exists() {
            if cli.overwrite {
                fs::remove_file(&ifc_path)
                    .with_context(|| format!("刪除既有 IFC 失敗：{}", ifc_path.display()))?;
            } else {
                bail!("IFC 已存在，請加 --overwrite：{}", ifc_path.display());
            }
        }
        input = export_rvt_to_ifc(&RvtToIfcOptions {
            input_rvt: input,
            output_ifc: ifc_path,
            requested_version: cli.revit_version.requested(),
            revit_exe: cli.revit_exe.clone(),
            bridge_assembly: cli.bridge_assembly.clone(),
            timeout: Duration::from_secs(rvt_timeout_secs),
        })?;
        if !cli.keep_ifc {
            cleanup_after_convert.push(input.clone());
            cleanup_after_convert.push(input.with_extension("rvt-export-job.json"));
            cleanup_after_convert.push(input.with_extension("rvt-export-result.json"));
        }
    }

    let outputs = convert_path(&ConvertOptions {
        input,
        output,
        source_epsg: cli.source_epsg,
        tile_max_features: cli.tile_max_features,
        tile_max_triangles: cli.tile_max_triangles,
        normal_mode: cli.normal_mode.into(),
        smooth_angle_deg: cli.smooth_angle_deg,
        overwrite: cli.overwrite,
    })?;
    for path in cleanup_after_convert {
        let _ = fs::remove_file(path);
    }
    for output in outputs {
        println!("{}", output.display());
    }
    Ok(())
}

fn run_command(command: &Command) -> Result<()> {
    match command {
        Command::Inspect {
            input,
            output,
            source_epsg,
        } => run_inspect(input, output, *source_epsg),
        Command::EntityInspectDxf {
            conversion_report,
            manifest,
            output,
            batch_size,
            ogrinfo,
        } => run_entity_inspect_dxf(conversion_report, manifest, output, *batch_size, ogrinfo),
        Command::InspectReview {
            input,
            output,
            db,
            manifest,
        } => run_inspect_review(input, output, db, manifest),
        Command::InspectDrilldown {
            input,
            output,
            db,
            manifest,
        } => run_inspect_drilldown(input, output, db, manifest),
        Command::PublishApproved { input, output } => run_publish_approved(input, output),
        Command::RuntimePublish { input, output } => run_runtime_publish(input, output),
        Command::GeometryPreview { input, output } => run_geometry_preview(input, output),
        Command::IfcInfo { input, output } => run_ifc_info(input, output),
    }
}

fn run_ifc_info(input: &Path, output: &Path) -> Result<()> {
    let outputs = write_ifc_info_path(input, output)?;
    for output in outputs {
        println!("{}", output.display());
    }
    Ok(())
}

fn run_inspect(input: &Path, output: &Path, source_epsg: u32) -> Result<()> {
    fs::create_dir_all(output)
        .with_context(|| format!("建立 inspect 輸出目錄失敗：{}", output.display()))?;
    let mut sources = discover_sources(input)?;
    if let Err(err) = ifc_to_3dtiles::inspect::write_empty_cad_metadata_dumps(&mut sources, output)
    {
        log::warn!("CAD metadata sidecar 產生失敗：{err:#}");
    }
    let anchor_source_id = sources
        .iter()
        .find(|source| source.format == SourceFormat::Ifc)
        .map(|source| source.id.clone());
    let manifest = ProjectManifest {
        project_id: safe_stem(input),
        source_epsg,
        anchor_source_id,
        allowed_scales: vec![1000.0, 1.0, 0.1, 0.01, 0.001],
        sources,
    };
    let path = output.join("source_manifest.json");
    fs::write(&path, serde_json::to_vec_pretty(&manifest)?)
        .with_context(|| format!("寫入 manifest 失敗：{}", path.display()))?;
    println!("{}", path.display());
    Ok(())
}

fn run_entity_inspect_dxf(
    conversion_report: &Path,
    manifest_path: &Path,
    output: &Path,
    batch_size: usize,
    ogrinfo: &Option<PathBuf>,
) -> Result<()> {
    fs::create_dir_all(output)
        .with_context(|| format!("建立 entity inspect 輸出目錄失敗：{}", output.display()))?;
    let conversion: CadConversionReport =
        serde_json::from_slice(&fs::read(conversion_report).with_context(|| {
            format!(
                "讀取 conversion report 失敗：{}",
                conversion_report.display()
            )
        })?)
        .with_context(|| {
            format!(
                "解析 conversion report 失敗：{}",
                conversion_report.display()
            )
        })?;
    let mut manifest: ProjectManifest = serde_json::from_slice(
        &fs::read(manifest_path)
            .with_context(|| format!("讀取 manifest 失敗：{}", manifest_path.display()))?,
    )
    .with_context(|| format!("解析 manifest 失敗：{}", manifest_path.display()))?;
    let ogrinfo_path = resolve_ogrinfo(ogrinfo)?;
    let entity_dir = output.join("cad_entities");
    fs::create_dir_all(&entity_dir)
        .with_context(|| format!("建立 entity JSONL 目錄失敗：{}", entity_dir.display()))?;

    let mut all_entities = Vec::<CadEntity>::new();
    let mut source_stats = Vec::new();
    let mut report_warnings = Vec::new();
    for entry in conversion.entries.iter().filter(|entry| {
        entry.success
            && entry
                .converted_format
                .as_deref()
                .is_some_and(|format| format.eq_ignore_ascii_case("dxf"))
    }) {
        let Some(converted_path) = &entry.converted_path else {
            continue;
        };
        println!("entity inspect {}", entry.source_original_file_name);
        let entities =
            read_dxf_entities(&ogrinfo_path, converted_path, &entry.source_id, batch_size)?;
        let jsonl_path = entity_dir.join(format!("{}.entities.jsonl", entry.source_id));
        write_entities_jsonl(&jsonl_path, &entities)?;
        let stats = summarize_entities(&entry.source_id, &entities, &manifest.allowed_scales);
        update_manifest_source(&mut manifest, &entry.source_id, &stats);
        all_entities.extend(entities);
        source_stats.push(stats);
    }

    for source in &mut manifest.sources {
        if source.format == SourceFormat::Dgn {
            source.status = SourceStatus::NeedsAlternativeRoute;
            source.inspect_status = Some("needs_alternative_route".to_string());
            if !source
                .warnings
                .iter()
                .any(|warning| warning.contains("ODA invalid group code"))
            {
                source
                    .warnings
                    .push("DGN needs alternative route: ODA invalid group code".to_string());
            }
        }
    }

    let db_path = output.join("project_inspect.db");
    write_entity_inspect_db(&db_path, &all_entities, &source_stats)?;
    write_manifest_sources_and_conversion_runs(&db_path, &manifest, &conversion)?;
    let report = EntityInspectReport {
        generated_at: chrono_like_now(),
        conversion_report_path: conversion_report.to_path_buf(),
        project_db_path: db_path.clone(),
        entity_count: all_entities.len() as u64,
        parsed_entity_count: all_entities
            .iter()
            .filter(|entity| entity.bbox.is_some())
            .count() as u64,
        skipped_entity_count: all_entities
            .iter()
            .filter(|entity| entity.bbox.is_none())
            .count() as u64,
        sources: source_stats,
        warnings: std::mem::take(&mut report_warnings),
    };
    let report_path = output.join("entity_inspect_report.json");
    fs::write(&report_path, serde_json::to_vec_pretty(&report)?)
        .with_context(|| format!("寫入 entity inspect report 失敗：{}", report_path.display()))?;
    fs::write(manifest_path, serde_json::to_vec_pretty(&manifest)?)
        .with_context(|| format!("回填 manifest 失敗：{}", manifest_path.display()))?;
    println!("{}", report_path.display());
    println!("{}", db_path.display());
    Ok(())
}

fn run_inspect_review(
    input: &Path,
    output: &Option<PathBuf>,
    db: &Option<PathBuf>,
    manifest: &Option<PathBuf>,
) -> Result<()> {
    let db_path = db
        .clone()
        .unwrap_or_else(|| input.join("project_inspect.db"));
    let manifest_path = manifest
        .clone()
        .unwrap_or_else(|| input.join("source_manifest.json"));
    let output_path = output
        .clone()
        .unwrap_or_else(|| input.join("review_report.html"));
    write_review_report_html(&db_path, &manifest_path, &output_path)?;
    println!("{}", output_path.display());
    Ok(())
}

fn run_inspect_drilldown(
    input: &Path,
    output: &Option<PathBuf>,
    db: &Option<PathBuf>,
    manifest: &Option<PathBuf>,
) -> Result<()> {
    let db_path = db
        .clone()
        .unwrap_or_else(|| input.join("project_inspect.db"));
    let manifest_path = manifest
        .clone()
        .unwrap_or_else(|| input.join("source_manifest.json"));
    let output_dir = output.clone().unwrap_or_else(|| input.join("qa"));
    let review_report_path = input.join("review_report.html");
    write_drilldown_outputs(&db_path, &manifest_path, &output_dir, &review_report_path)?;
    println!("{}", output_dir.display());
    println!("{}", review_report_path.display());
    Ok(())
}

fn run_publish_approved(input: &Path, output: &Option<PathBuf>) -> Result<()> {
    let output_path = output.clone().unwrap_or_else(|| input.join("publish"));
    write_publish_skeleton_outputs(input, &output_path)?;
    println!("{}", output_path.display());
    Ok(())
}

fn run_runtime_publish(input: &Path, output: &Option<PathBuf>) -> Result<()> {
    let output_path = output.clone().unwrap_or_else(|| input.join("publish"));
    write_runtime_publish_outputs(input, &output_path)?;
    println!("{}", output_path.join("runtime_manifest.json").display());
    println!(
        "{}",
        output_path.join("runtime_budget_report.json").display()
    );
    Ok(())
}

fn run_geometry_preview(input: &Path, output: &Option<PathBuf>) -> Result<()> {
    let output_path = output.clone().unwrap_or_else(|| input.join("publish"));
    write_geometry_preview_outputs(input, &output_path)?;
    println!(
        "{}",
        output_path
            .join("geometry_preview")
            .join("geometry_publish_report.json")
            .display()
    );
    println!(
        "{}",
        output_path
            .join("geometry_preview")
            .join("tileset.json")
            .display()
    );
    Ok(())
}

fn write_manifest_sources_and_conversion_runs(
    db_path: &Path,
    manifest: &ProjectManifest,
    conversion: &CadConversionReport,
) -> Result<()> {
    let conn = rusqlite::Connection::open(db_path)?;
    for source in &manifest.sources {
        conn.execute(
            "INSERT OR REPLACE INTO sources(source_id, inspect_status, selected_scale, fingerprint_hash)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                source.id,
                source
                    .inspect_status
                    .clone()
                    .unwrap_or_else(|| source_status_text(source.status).to_string()),
                source.selected_scale,
                source.fingerprint_hash
            ],
        )?;
    }
    for entry in &conversion.entries {
        conn.execute(
            "INSERT INTO conversion_runs(source_id, target_version, target_format, success, report_json)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                entry.source_id,
                entry.target_version,
                entry.target_format,
                entry.success,
                serde_json::to_string(entry)?,
            ],
        )?;
    }
    Ok(())
}

fn source_status_text(status: SourceStatus) -> &'static str {
    match status {
        SourceStatus::PendingInspect => "pending_inspect",
        SourceStatus::NeedsAlternativeRoute => "needs_alternative_route",
        SourceStatus::Approved => "approved",
        SourceStatus::Quarantined => "quarantined",
        SourceStatus::Converted => "converted",
        SourceStatus::Published => "published",
    }
}

fn read_dxf_entities(
    ogrinfo: &Path,
    dxf_path: &Path,
    source_id: &str,
    batch_size: usize,
) -> Result<Vec<CadEntity>> {
    if batch_size == 0 {
        let output = ProcessCommand::new(ogrinfo)
            .arg(dxf_path)
            .arg("entities")
            .arg("-geom=YES")
            .output()
            .with_context(|| format!("執行 ogrinfo 失敗：{}", dxf_path.display()))?;
        if !output.status.success() {
            bail!(
                "ogrinfo failed for {}: {}",
                dxf_path.display(),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        return Ok(parse_ogrinfo_entities(
            source_id,
            &String::from_utf8_lossy(&output.stdout),
        ));
    }

    let mut offset = 0_usize;
    let mut entities = Vec::new();
    loop {
        let sql = format!(
            "SELECT FID, Layer, SubClasses, Linetype, EntityHandle FROM entities LIMIT {batch_size} OFFSET {offset}"
        );
        let output = ProcessCommand::new(ogrinfo)
            .arg(dxf_path)
            .arg("-sql")
            .arg(sql)
            .arg("-geom=YES")
            .output()
            .with_context(|| format!("執行 ogrinfo batch 失敗：{}", dxf_path.display()))?;
        if !output.status.success() {
            bail!(
                "ogrinfo batch failed for {}: {}",
                dxf_path.display(),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let batch = parse_ogrinfo_entities(source_id, &String::from_utf8_lossy(&output.stdout));
        let count = batch.len();
        entities.extend(batch);
        if count < batch_size {
            break;
        }
        offset += batch_size;
    }
    Ok(entities)
}

fn update_manifest_source(
    manifest: &mut ProjectManifest,
    source_id: &str,
    stats: &ifc_to_3dtiles::cad_entity_inspect::CadEntityStats,
) {
    if let Some(source) = manifest
        .sources
        .iter_mut()
        .find(|source| source.id == source_id)
    {
        source.status = if stats.inspect_status == "approved" {
            SourceStatus::Approved
        } else {
            SourceStatus::Quarantined
        };
        source.inspect_status = Some(stats.inspect_status.clone());
        source.selected_scale = stats.selected_scale;
        source.unit_scale_to_meter = stats.selected_scale;
        source.raw_bbox = Some(stats.raw_bbox);
        source.percentile_bbox = Some(stats.percentile_bbox);
        source.fingerprint_hash = Some(stats.fingerprint_hash.clone());
        for warning in &stats.warnings {
            if !source.warnings.contains(warning) {
                source.warnings.push(warning.clone());
            }
        }
    }
}

fn write_entities_jsonl(path: &Path, entities: &[CadEntity]) -> Result<()> {
    let mut text = String::new();
    for entity in entities {
        text.push_str(&serde_json::to_string(entity)?);
        text.push('\n');
    }
    fs::write(path, text).with_context(|| format!("寫入 entity JSONL 失敗：{}", path.display()))?;
    Ok(())
}

fn resolve_ogrinfo(override_path: &Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = override_path {
        return Ok(path.clone());
    }
    if let Ok(path) = std::env::var("OGRINFO_EXE") {
        return Ok(PathBuf::from(path));
    }
    for candidate in [
        r"C:\ms4w_MSSQL\GDAL\ogrinfo.exe",
        r"C:\ms4w\tools\gdal-ogr\ogrinfo.exe",
    ] {
        let path = PathBuf::from(candidate);
        if path.exists() {
            return Ok(path);
        }
    }
    Ok(PathBuf::from("ogrinfo.exe"))
}

fn chrono_like_now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| format!("unix:{}", duration.as_secs()))
        .unwrap_or_else(|_| "unix:0".to_string())
}

fn has_extension(path: &Path, extension: &str) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case(extension))
}

fn safe_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("model")
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ => ch,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_TILE_MAX_FEATURES, DEFAULT_TILE_MAX_TRIANGLES};

    #[test]
    fn default_tile_budget_targets_small_b3dm_files() {
        assert_eq!(DEFAULT_TILE_MAX_FEATURES, 50);
        assert_eq!(DEFAULT_TILE_MAX_TRIANGLES, 20_000);
    }
}
