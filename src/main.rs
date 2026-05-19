use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use clap::{Parser, ValueEnum};
use ifc_to_3dtiles::{
    ConvertOptions, NormalMode, convert_path,
    revit::RevitVersion,
    rvt::{RvtToIfcOptions, export_rvt_to_ifc},
};

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
    #[arg(long)]
    input: PathBuf,

    #[arg(long)]
    output: PathBuf,

    #[arg(long, default_value_t = 3826)]
    source_epsg: u32,

    #[arg(long, default_value_t = 500)]
    tile_max_features: usize,

    #[arg(long, default_value_t = 200000)]
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

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let cli = Cli::parse();
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

    let mut input = cli.input.clone();
    let mut cleanup_after_convert = Vec::new();
    if has_extension(&input, "rvt") {
        fs::create_dir_all(&cli.output)
            .with_context(|| format!("建立輸出目錄失敗：{}", cli.output.display()))?;
        let ifc_path = cli.output.join(format!("{}.ifc", safe_stem(&input)));
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
        output: cli.output,
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
