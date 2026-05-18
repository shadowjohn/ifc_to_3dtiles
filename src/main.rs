use std::path::PathBuf;

use anyhow::{Result, ensure};
use clap::{Parser, ValueEnum};
use ifc_to_3dtiles::{ConvertOptions, NormalMode, convert_path};

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

#[derive(Debug, Parser)]
#[command(name = "ifc_to_3dtiles")]
#[command(about = "Convert AECOsim IFC2X3 FacetedBRep models to Cesium 3D Tiles")]
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
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let cli = Cli::parse();
    ensure!(
        (0.0..=180.0).contains(&cli.smooth_angle_deg),
        "--smooth-angle-deg 必須介於 0 到 180"
    );
    let outputs = convert_path(&ConvertOptions {
        input: cli.input,
        output: cli.output,
        source_epsg: cli.source_epsg,
        tile_max_features: cli.tile_max_features,
        tile_max_triangles: cli.tile_max_triangles,
        normal_mode: cli.normal_mode.into(),
        smooth_angle_deg: cli.smooth_angle_deg,
        overwrite: cli.overwrite,
    })?;
    for output in outputs {
        println!("{}", output.display());
    }
    Ok(())
}
