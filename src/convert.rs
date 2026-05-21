use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use log::info;
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::{
    b3dm, crs,
    geometry::{
        Bounds, Mat4, Mesh, MeshBuildOptions, Vec3, axis2_placement_3d, cartesian_operator_3d,
        local_placement_matrix, mesh_extruded_area_solid, mesh_faceted_brep,
    },
    glb,
    model::StyleTable,
    step::{
        EntityRecord, StepIndex, decode_ifc_string, extract_first_ref, extract_refs, numbers_in,
        split_arguments,
    },
    tiles::{self, TileJson},
};

type PsetValues = Map<String, Value>;
type PsetsByObject = HashMap<u32, Vec<(String, PsetValues)>>;

#[derive(Debug, Clone)]
pub struct ConvertOptions {
    pub input: PathBuf,
    pub output: PathBuf,
    pub source_epsg: u32,
    pub tile_max_features: usize,
    pub tile_max_triangles: usize,
    pub normal_mode: NormalMode,
    pub smooth_angle_deg: f64,
    pub overwrite: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormalMode {
    Flat,
    Smooth,
    Both,
}

impl NormalMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Flat => "flat",
            Self::Smooth => "smooth",
            Self::Both => "both",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FeatureMetadata {
    pub batch_id: u32,
    pub ifc_step_id: u32,
    pub global_id: String,
    pub ifc_type: String,
    pub name: String,
    pub description: String,
    pub dgn_element: String,
    pub site: String,
    pub building: String,
    pub storey: String,
    pub group_names: Vec<String>,
    pub style_id: String,
    pub color_rgba: [f32; 4],
    pub psets_json: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConversionReport {
    pub input_file: String,
    pub output_dir: String,
    pub source_epsg: u32,
    pub entity_count: usize,
    pub feature_count: usize,
    pub converted_features: usize,
    pub skipped_features: usize,
    pub style_item_count: usize,
    pub tile_count: usize,
    pub smooth_tile_count: usize,
    pub normal_mode: String,
    pub smooth_angle_deg: f64,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
struct Feature {
    metadata: FeatureMetadata,
    mesh: Mesh,
    unsupported_items: BTreeMap<String, usize>,
}

#[derive(Debug)]
enum FeatureBuildResult {
    Converted(Feature),
    Empty(ResolveStats),
}

#[derive(Debug, Default)]
struct TileWriteResult {
    flat_children: Vec<TileJson>,
    smooth_children: Vec<TileJson>,
}

#[derive(Debug, Default)]
struct ResolveStats {
    missing_color_faces: usize,
    unsupported_items: BTreeMap<String, usize>,
}

#[derive(Debug, Default)]
struct MeshResolve {
    mesh: Mesh,
    first_style_id: Option<u32>,
    first_color: Option<[f32; 4]>,
    stats: ResolveStats,
}

#[derive(Debug, Default)]
struct ModelContext {
    group_names_by_object: HashMap<u32, Vec<String>>,
    contained_in: HashMap<u32, u32>,
    parent_by_child: HashMap<u32, u32>,
    names: HashMap<u32, String>,
    types: HashMap<u32, String>,
    psets_by_object: PsetsByObject,
}

pub fn convert_path(options: &ConvertOptions) -> Result<Vec<PathBuf>> {
    let mut inputs = Vec::new();
    if options.input.is_file() {
        inputs.push(options.input.clone());
    } else {
        for entry in fs::read_dir(&options.input)
            .with_context(|| format!("讀取輸入目錄失敗：{}", options.input.display()))?
        {
            let path = entry?.path();
            if path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("ifc"))
            {
                inputs.push(path);
            }
        }
    }
    inputs.sort();
    if inputs.is_empty() {
        bail!("找不到 IFC 檔案：{}", options.input.display());
    }

    fs::create_dir_all(&options.output)
        .with_context(|| format!("建立輸出目錄失敗：{}", options.output.display()))?;

    let mut outputs = Vec::new();
    for input in inputs {
        outputs.push(convert_file(&input, options)?);
    }
    Ok(outputs)
}

fn convert_file(input: &Path, options: &ConvertOptions) -> Result<PathBuf> {
    info!("讀取 IFC：{}", input.display());
    let content =
        fs::read_to_string(input).with_context(|| format!("讀取 IFC 失敗：{}", input.display()))?;
    let index = StepIndex::parse(content);
    info!("建立 STEP index：{} entities", index.len());

    let styles = StyleTable::from_index(&index);
    let ctx = build_model_context(&index);
    let mut warnings = Vec::new();
    let mut features = Vec::new();
    let mut skipped = 0usize;
    let mut skipped_unsupported_items = BTreeMap::<String, usize>::new();

    let product_entities: Vec<&EntityRecord> = index
        .entities()
        .filter(|entity| is_ifc_product_with_shape(&index, entity))
        .collect();

    for (ordinal, entity) in product_entities.iter().enumerate() {
        match build_feature(&index, &ctx, &styles, entity, ordinal as u32) {
            Ok(FeatureBuildResult::Converted(feature)) => features.push(feature),
            Ok(FeatureBuildResult::Empty(stats)) => {
                skipped += 1;
                merge_unsupported_items(&mut skipped_unsupported_items, stats.unsupported_items);
            }
            Err(err) => {
                skipped += 1;
                if warnings.len() < 200 {
                    warnings.push(format!("#{} {}: {err}", entity.id, entity.type_name));
                }
            }
        }
        if (ordinal + 1) % 1000 == 0 {
            info!("已處理 {} 個 IFC product", ordinal + 1);
        }
    }

    if features.is_empty() {
        bail!("沒有可轉換的 IFC product 幾何");
    }

    let mut source_bounds = Bounds::empty();
    let mut centers = Vec::with_capacity(features.len());
    for feature in &features {
        source_bounds.include_bounds(&feature.mesh.bounds);
        let center = feature.mesh.bounds.center();
        centers.push([center.x, center.y, center.z]);
    }
    let source_origin = median_origin(&centers);
    let far_feature_count = centers
        .iter()
        .filter(|center| {
            let dx = center[0] - source_origin.x;
            let dy = center[1] - source_origin.y;
            (dx * dx + dy * dy).sqrt() > 10_000.0
        })
        .count();
    if far_feature_count > 0 {
        warnings.push(format!(
            "{far_feature_count} features 距離 median georef origin 超過 10km，已保留為遠距 tile"
        ));
    }
    let origin_geo = crs::project_to_wgs84(options.source_epsg, source_origin.x, source_origin.y)?;
    let root_transform =
        crs::enu_to_ecef_transform(origin_geo.lon_deg, origin_geo.lat_deg, source_origin.z);

    let mut root_bounds = Bounds::empty();
    for feature in &mut features {
        feature.mesh.translate(Vec3::new(
            -source_origin.x,
            -source_origin.y,
            -source_origin.z,
        ));
        root_bounds.include_bounds(&feature.mesh.bounds);
    }

    let output_dir = options.output.join(safe_stem(input));
    if output_dir.exists() {
        if options.overwrite {
            fs::remove_dir_all(&output_dir)
                .with_context(|| format!("清除既有輸出失敗：{}", output_dir.display()))?;
        } else {
            bail!("輸出目錄已存在，請加 --overwrite：{}", output_dir.display());
        }
    }
    let tiles_dir = output_dir.join("tiles");
    fs::create_dir_all(&tiles_dir)?;
    let smooth_tiles_dir = if options.normal_mode == NormalMode::Both {
        let dir = output_dir.join("tiles_smooth");
        fs::create_dir_all(&dir)?;
        Some(dir)
    } else {
        None
    };

    write_standalone_glbs(&output_dir, input, &features, options)?;
    fs::write(
        output_dir.join("metadata.json"),
        serde_json::to_vec_pretty(&build_metadata_report(&features))?,
    )?;
    fs::write(
        output_dir.join("unsupported_geometry_report.json"),
        serde_json::to_vec_pretty(&build_unsupported_geometry_report(
            &features,
            &skipped_unsupported_items,
        ))?,
    )?;

    let tile_result = write_tiles(
        &tiles_dir,
        smooth_tiles_dir.as_deref(),
        &mut features,
        options,
    )?;
    let tileset =
        tiles::build_tileset_json(root_transform, &root_bounds, &tile_result.flat_children);
    fs::write(
        output_dir.join("tileset.json"),
        serde_json::to_vec_pretty(&tileset)?,
    )?;
    if options.normal_mode == NormalMode::Both {
        let smooth_tileset =
            tiles::build_tileset_json(root_transform, &root_bounds, &tile_result.smooth_children);
        fs::write(
            output_dir.join("tileset_smooth.json"),
            serde_json::to_vec_pretty(&smooth_tileset)?,
        )?;
        if is_default_smooth_90(options.smooth_angle_deg) {
            write_smooth_90_alias(
                &output_dir,
                root_transform,
                &root_bounds,
                &tile_result.smooth_children,
            )?;
        }
    }

    let missing_color_total: usize = features
        .iter()
        .filter(|feature| feature.metadata.style_id.is_empty())
        .count();
    if missing_color_total > 0 {
        warnings.push(format!(
            "{missing_color_total} features 使用 fallback 顏色，IFC style 未完整對應"
        ));
    }

    let report = ConversionReport {
        input_file: input.display().to_string(),
        output_dir: output_dir.display().to_string(),
        source_epsg: options.source_epsg,
        entity_count: index.len(),
        feature_count: features.len() + skipped,
        converted_features: features.len(),
        skipped_features: skipped,
        style_item_count: styles.len(),
        tile_count: tile_result.flat_children.len(),
        smooth_tile_count: if options.normal_mode == NormalMode::Smooth {
            tile_result.flat_children.len()
        } else {
            tile_result.smooth_children.len()
        },
        normal_mode: options.normal_mode.as_str().to_string(),
        smooth_angle_deg: if options.normal_mode == NormalMode::Flat {
            0.0
        } else {
            options.smooth_angle_deg
        },
        warnings,
    };
    fs::write(
        output_dir.join("conversion_report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;

    info!("輸出完成：{}", output_dir.display());
    Ok(output_dir)
}

fn write_tiles(
    tiles_dir: &Path,
    smooth_tiles_dir: Option<&Path>,
    features: &mut [Feature],
    options: &ConvertOptions,
) -> Result<TileWriteResult> {
    sort_features_spatially(features);
    let max_features = options.tile_max_features.max(1);
    let max_triangles = options.tile_max_triangles.max(1);
    let mut result = TileWriteResult::default();
    let mut start = 0usize;
    let mut tile_index = 0usize;

    while start < features.len() {
        let first_triangles = features[start].mesh.triangle_count();
        if first_triangles > max_triangles {
            let feature = &features[start];
            let mut triangle_start = 0usize;
            while triangle_start < first_triangles {
                let triangle_end = (triangle_start + max_triangles).min(first_triangles);
                let tile_mesh = mesh_triangle_range(&feature.mesh, triangle_start, triangle_end, 0);
                let mut meta = feature.metadata.clone();
                meta.batch_id = 0;
                write_tile_outputs(
                    tiles_dir,
                    smooth_tiles_dir,
                    &mut result,
                    tile_index,
                    &tile_mesh,
                    &[meta],
                    options,
                )?;
                tile_index += 1;
                triangle_start = triangle_end;
            }
            start += 1;
            continue;
        }

        let mut end = start;
        let mut tri_count = 0usize;
        while end < features.len() && end - start < max_features {
            let next_triangles = features[end].mesh.triangle_count();
            if end > start && tri_count + next_triangles > max_triangles {
                break;
            }
            tri_count += next_triangles;
            end += 1;
        }
        if end == start {
            end += 1;
        }

        let chunk = &features[start..end];
        let mut tile_mesh = Mesh::new();
        let mut metadata = Vec::with_capacity(chunk.len());
        for (local_id, feature) in chunk.iter().enumerate() {
            tile_mesh.append_with_batch(&feature.mesh, local_id as u16);
            let mut meta = feature.metadata.clone();
            meta.batch_id = local_id as u32;
            metadata.push(meta);
        }

        write_tile_outputs(
            tiles_dir,
            smooth_tiles_dir,
            &mut result,
            tile_index,
            &tile_mesh,
            &metadata,
            options,
        )?;

        tile_index += 1;
        start = end;
    }

    Ok(result)
}

fn write_tile_outputs(
    tiles_dir: &Path,
    smooth_tiles_dir: Option<&Path>,
    result: &mut TileWriteResult,
    tile_index: usize,
    tile_mesh: &Mesh,
    metadata: &[FeatureMetadata],
    options: &ConvertOptions,
) -> Result<()> {
    let batch_table = build_batch_table(metadata);
    let filename = format!("tile_{tile_index:04}.b3dm");
    if options.normal_mode == NormalMode::Smooth {
        let smooth_mesh =
            tile_mesh.with_smoothed_normals_by_position_angle(1e-6, options.smooth_angle_deg);
        write_b3dm(
            tiles_dir,
            &filename,
            &smooth_mesh,
            metadata.len(),
            &batch_table,
        )?;
    } else {
        write_b3dm(
            tiles_dir,
            &filename,
            tile_mesh,
            metadata.len(),
            &batch_table,
        )?;
    }
    result.flat_children.push(TileJson {
        uri: format!("tiles/{filename}"),
        bounds: tile_mesh.bounds,
        geometric_error: 0.0,
    });

    if let Some(smooth_tiles_dir) = smooth_tiles_dir {
        let smooth_mesh =
            tile_mesh.with_smoothed_normals_by_position_angle(1e-6, options.smooth_angle_deg);
        write_b3dm(
            smooth_tiles_dir,
            &filename,
            &smooth_mesh,
            metadata.len(),
            &batch_table,
        )?;
        result.smooth_children.push(TileJson {
            uri: format!("tiles_smooth/{filename}"),
            bounds: smooth_mesh.bounds,
            geometric_error: 0.0,
        });
    }

    Ok(())
}

fn mesh_triangle_range(
    source: &Mesh,
    triangle_start: usize,
    triangle_end: usize,
    batch_id: u16,
) -> Mesh {
    let mut mesh = Mesh::new();
    let vertex_start = triangle_start * 3;
    let vertex_end = triangle_end * 3;
    for index in vertex_start..vertex_end {
        if let Some(position) = source.positions.get(index) {
            mesh.positions.push(*position);
            mesh.bounds
                .include(Vec3::new(position[0], position[1], position[2]));
        }
        if let Some(normal) = source.normals.get(index) {
            mesh.normals.push(*normal);
        }
        if let Some(color) = source.colors.get(index) {
            mesh.colors.push(*color);
        }
        mesh.batch_ids.push(batch_id);
    }
    mesh
}

fn write_smooth_90_alias(
    output_dir: &Path,
    root_transform: [f64; 16],
    root_bounds: &Bounds,
    smooth_children: &[TileJson],
) -> Result<()> {
    let smooth_dir = output_dir.join("tiles_smooth");
    let smooth_90_dir = output_dir.join("tiles_smooth_90");
    copy_dir_contents(&smooth_dir, &smooth_90_dir)?;

    let smooth_90_children =
        retarget_tile_uris(smooth_children, "tiles_smooth/", "tiles_smooth_90/");
    let smooth_90_tileset =
        tiles::build_tileset_json(root_transform, root_bounds, &smooth_90_children);
    fs::write(
        output_dir.join("tileset_smooth_90.json"),
        serde_json::to_vec_pretty(&smooth_90_tileset)?,
    )?;
    Ok(())
}

fn is_default_smooth_90(angle: f64) -> bool {
    (angle - 90.0).abs() < 1e-9
}

fn retarget_tile_uris(children: &[TileJson], from: &str, to: &str) -> Vec<TileJson> {
    children
        .iter()
        .map(|child| {
            let mut retargeted = child.clone();
            if let Some(rest) = retargeted.uri.strip_prefix(from) {
                retargeted.uri = format!("{to}{rest}");
            }
            retargeted
        })
        .collect()
}

fn copy_dir_contents(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)
        .with_context(|| format!("建立目錄失敗：{}", destination.display()))?;
    for entry in
        fs::read_dir(source).with_context(|| format!("讀取目錄失敗：{}", source.display()))?
    {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let target = destination.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_contents(&entry.path(), &target)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), &target)
                .with_context(|| format!("複製檔案失敗：{}", target.display()))?;
        }
    }
    Ok(())
}

fn write_standalone_glbs(
    output_dir: &Path,
    input: &Path,
    features: &[Feature],
    options: &ConvertOptions,
) -> Result<()> {
    let mut mesh = Mesh::new();
    for (batch_id, feature) in features.iter().enumerate() {
        mesh.append_with_batch(&feature.mesh, batch_id as u16);
    }
    let metadata = build_metadata_report(features);
    let stem = safe_stem(input);

    if matches!(options.normal_mode, NormalMode::Flat | NormalMode::Both) {
        let extras = standalone_glb_extras(input, "flat", features.len(), &metadata);
        fs::write(
            output_dir.join(format!("{stem}_flat.glb")),
            glb::build_glb_with_extras(&mesh, Some(extras))?,
        )?;
    }
    if matches!(options.normal_mode, NormalMode::Smooth | NormalMode::Both) {
        let extras = standalone_glb_extras(input, "smooth", features.len(), &metadata);
        let smooth = mesh.with_smoothed_normals_by_position_angle(1e-6, options.smooth_angle_deg);
        fs::write(
            output_dir.join(format!("{stem}_smooth.glb")),
            glb::build_glb_with_extras(&smooth, Some(extras))?,
        )?;
    }
    Ok(())
}

fn standalone_glb_extras(
    input: &Path,
    normal_mode: &str,
    feature_count: usize,
    metadata: &Value,
) -> Value {
    json!({
        "source": input.display().to_string(),
        "metadataFile": "metadata.json",
        "featureCount": feature_count,
        "normalMode": normal_mode,
        "features": metadata,
    })
}

fn write_b3dm(
    directory: &Path,
    filename: &str,
    mesh: &Mesh,
    batch_length: usize,
    batch_table: &Value,
) -> Result<()> {
    let glb = glb::build_glb(mesh)?;
    let b3dm = b3dm::build_b3dm(&glb, batch_length, batch_table)?;
    fs::write(directory.join(filename), b3dm)?;
    Ok(())
}

fn build_batch_table(metadata: &[FeatureMetadata]) -> Value {
    let mut object = Map::new();
    object.insert(
        "batch_id".to_string(),
        Value::Array(metadata.iter().map(|m| json!(m.batch_id)).collect()),
    );
    object.insert(
        "ifc_step_id".to_string(),
        Value::Array(metadata.iter().map(|m| json!(m.ifc_step_id)).collect()),
    );
    object.insert(
        "global_id".to_string(),
        Value::Array(metadata.iter().map(|m| json!(m.global_id)).collect()),
    );
    object.insert(
        "ifc_type".to_string(),
        Value::Array(metadata.iter().map(|m| json!(m.ifc_type)).collect()),
    );
    object.insert(
        "name".to_string(),
        Value::Array(metadata.iter().map(|m| json!(m.name)).collect()),
    );
    object.insert(
        "description".to_string(),
        Value::Array(metadata.iter().map(|m| json!(m.description)).collect()),
    );
    object.insert(
        "dgn_element".to_string(),
        Value::Array(metadata.iter().map(|m| json!(m.dgn_element)).collect()),
    );
    object.insert(
        "site".to_string(),
        Value::Array(metadata.iter().map(|m| json!(m.site)).collect()),
    );
    object.insert(
        "building".to_string(),
        Value::Array(metadata.iter().map(|m| json!(m.building)).collect()),
    );
    object.insert(
        "storey".to_string(),
        Value::Array(metadata.iter().map(|m| json!(m.storey)).collect()),
    );
    object.insert(
        "group_names".to_string(),
        Value::Array(metadata.iter().map(|m| json!(m.group_names)).collect()),
    );
    object.insert(
        "style_id".to_string(),
        Value::Array(metadata.iter().map(|m| json!(m.style_id)).collect()),
    );
    object.insert(
        "color_rgba".to_string(),
        Value::Array(metadata.iter().map(|m| json!(m.color_rgba)).collect()),
    );
    object.insert(
        "psets_json".to_string(),
        Value::Array(metadata.iter().map(|m| json!(m.psets_json)).collect()),
    );
    Value::Object(object)
}

fn build_metadata_report(features: &[Feature]) -> Value {
    Value::Array(
        features
            .iter()
            .map(|feature| {
                let mut value = serde_json::to_value(&feature.metadata).unwrap_or(Value::Null);
                if let Value::Object(object) = &mut value {
                    let psets = serde_json::from_str::<Value>(&feature.metadata.psets_json)
                        .unwrap_or_else(|_| json!({}));
                    object.insert("psets".to_string(), psets);
                }
                value
            })
            .collect(),
    )
}

fn build_unsupported_geometry_report(
    features: &[Feature],
    skipped_unsupported_items: &BTreeMap<String, usize>,
) -> Value {
    let mut unsupported = BTreeMap::<String, usize>::new();
    merge_unsupported_items(&mut unsupported, skipped_unsupported_items.clone());
    for feature in features {
        for (key, value) in &feature.unsupported_items {
            *unsupported.entry(key.clone()).or_insert(0) += value;
        }
    }
    json!({
        "feature_count": features.len(),
        "unsupported_items": unsupported,
    })
}

fn merge_unsupported_items(target: &mut BTreeMap<String, usize>, source: BTreeMap<String, usize>) {
    for (key, value) in source {
        *target.entry(key).or_insert(0) += value;
    }
}

pub fn median_origin(points: &[[f64; 3]]) -> Vec3 {
    fn median(values: &mut [f64]) -> f64 {
        values.sort_by(|a, b| a.total_cmp(b));
        values[values.len() / 2]
    }

    if points.is_empty() {
        return Vec3::default();
    }
    let mut xs: Vec<f64> = points.iter().map(|p| p[0]).collect();
    let mut ys: Vec<f64> = points.iter().map(|p| p[1]).collect();
    let mut zs: Vec<f64> = points.iter().map(|p| p[2]).collect();
    Vec3::new(median(&mut xs), median(&mut ys), median(&mut zs))
}

fn sort_features_spatially(features: &mut [Feature]) {
    let mut bounds = Bounds::empty();
    for feature in features.iter() {
        bounds.include_bounds(&feature.mesh.bounds);
    }
    let size = bounds.size();
    let target_tiles = (features.len() as f64 / 500.0).ceil().max(1.0);
    let cell = ((size.x.abs() * size.y.abs()) / target_tiles)
        .sqrt()
        .max(1.0);
    features.sort_by(|a, b| {
        let ac = a.mesh.bounds.center();
        let bc = b.mesh.bounds.center();
        let ak = ((ac.x / cell).floor() as i64, (ac.y / cell).floor() as i64);
        let bk = ((bc.x / cell).floor() as i64, (bc.y / cell).floor() as i64);
        ak.cmp(&bk)
            .then_with(|| a.metadata.ifc_step_id.cmp(&b.metadata.ifc_step_id))
    });
}

fn build_feature(
    index: &StepIndex,
    ctx: &ModelContext,
    styles: &StyleTable,
    entity: &EntityRecord,
    ordinal: u32,
) -> Result<FeatureBuildResult> {
    let args = split_arguments(index.body(entity));
    let global_id = args
        .first()
        .map(|v| decode_ifc_string(v))
        .unwrap_or_default();
    let name = normalize_text(
        args.get(2)
            .map(|v| decode_ifc_string(v))
            .unwrap_or_default(),
    );
    let description = normalize_text(
        args.get(3)
            .map(|v| decode_ifc_string(v))
            .unwrap_or_default(),
    );
    let dgn_element = parse_dgn_element(&description);
    let placement_id = args
        .get(5)
        .and_then(|arg| extract_first_ref(arg))
        .ok_or_else(|| anyhow!("product has no ObjectPlacement"))?;
    let representation_id = args
        .get(6)
        .and_then(|arg| extract_first_ref(arg))
        .ok_or_else(|| anyhow!("product has no Representation"))?;
    let transform = local_placement_matrix(index, placement_id)?;
    let resolved = mesh_product_definition(index, representation_id, transform, styles)?;
    if resolved.mesh.is_empty() {
        return Ok(FeatureBuildResult::Empty(resolved.stats));
    }

    let storey_id = ctx.contained_in.get(&entity.id).copied();
    let building_id = storey_id.and_then(|id| find_ancestor(ctx, id, "IFCBUILDING"));
    let site_id = storey_id.and_then(|id| find_ancestor(ctx, id, "IFCSITE"));
    let psets = collect_psets_json(ctx, &[Some(entity.id), storey_id, building_id, site_id])?;

    let color = resolved.first_color.unwrap_or([0.65, 0.65, 0.65, 1.0]);
    let metadata = FeatureMetadata {
        batch_id: ordinal,
        ifc_step_id: entity.id,
        global_id,
        ifc_type: entity.type_name.clone(),
        name,
        description,
        dgn_element,
        site: site_id
            .and_then(|id| ctx.names.get(&id).cloned())
            .unwrap_or_default(),
        building: building_id
            .and_then(|id| ctx.names.get(&id).cloned())
            .unwrap_or_default(),
        storey: storey_id
            .and_then(|id| ctx.names.get(&id).cloned())
            .unwrap_or_default(),
        group_names: ctx
            .group_names_by_object
            .get(&entity.id)
            .cloned()
            .unwrap_or_default(),
        style_id: resolved
            .first_style_id
            .map(|id| format!("#{id}"))
            .unwrap_or_default(),
        color_rgba: color,
        psets_json: psets,
    };

    Ok(FeatureBuildResult::Converted(Feature {
        metadata,
        mesh: resolved.mesh,
        unsupported_items: resolved.stats.unsupported_items,
    }))
}

fn is_ifc_product_with_shape(index: &StepIndex, entity: &EntityRecord) -> bool {
    if matches!(
        entity.type_name.as_str(),
        "IFCPROJECT"
            | "IFCSITE"
            | "IFCBUILDING"
            | "IFCBUILDINGSTOREY"
            | "IFCGROUP"
            | "IFCPROPERTYSET"
            | "IFCPROPERTYSINGLEVALUE"
            | "IFCRELDEFINESBYPROPERTIES"
            | "IFCRELCONTAINEDINSPATIALSTRUCTURE"
            | "IFCRELAGGREGATES"
            | "IFCRELASSIGNSTOGROUP"
    ) {
        return false;
    }

    let args = split_arguments(index.body(entity));
    let Some(representation_id) = args.get(6).and_then(|arg| extract_first_ref(arg)) else {
        return false;
    };
    index
        .entity(representation_id)
        .is_some_and(|representation| representation.type_name == "IFCPRODUCTDEFINITIONSHAPE")
}

fn mesh_product_definition(
    index: &StepIndex,
    pds_id: u32,
    transform: Mat4,
    styles: &StyleTable,
) -> Result<MeshResolve> {
    let pds = index
        .entity(pds_id)
        .ok_or_else(|| anyhow!("missing product definition shape #{pds_id}"))?;
    let args = split_arguments(index.body(pds));
    let reps = args.get(2).map(|arg| extract_refs(arg)).unwrap_or_default();
    let mut resolved = MeshResolve::default();
    for rep_id in reps {
        let child = mesh_shape_representation(index, rep_id, transform, styles)?;
        merge_resolve(&mut resolved, child);
    }
    Ok(resolved)
}

fn mesh_shape_representation(
    index: &StepIndex,
    rep_id: u32,
    transform: Mat4,
    styles: &StyleTable,
) -> Result<MeshResolve> {
    let rep = index
        .entity(rep_id)
        .ok_or_else(|| anyhow!("missing shape representation #{rep_id}"))?;
    let args = split_arguments(index.body(rep));
    let identifier = args
        .get(1)
        .map(|arg| decode_ifc_string(arg))
        .unwrap_or_default();
    let rep_type = args
        .get(2)
        .map(|arg| decode_ifc_string(arg))
        .unwrap_or_default();
    if identifier.eq_ignore_ascii_case("Box") || rep_type.eq_ignore_ascii_case("BoundingBox") {
        return Ok(MeshResolve::default());
    }

    let mut resolved = MeshResolve::default();
    for item_id in args.get(3).map(|arg| extract_refs(arg)).unwrap_or_default() {
        let Some(item) = index.entity(item_id) else {
            continue;
        };
        match item.type_name.as_str() {
            "IFCFACETEDBREP" => {
                let color = styles
                    .color_for_item(item_id)
                    .unwrap_or([0.65, 0.65, 0.65, 1.0]);
                let item_transform =
                    if crate::geometry::item_uses_projected_coordinates(index, item_id) {
                        Mat4::identity()
                    } else {
                        transform
                    };
                let mesh = mesh_faceted_brep(
                    index,
                    item_id,
                    &item_transform,
                    MeshBuildOptions { batch_id: 0, color },
                )?;
                if !mesh.is_empty() {
                    if styles.color_for_item(item_id).is_some() && resolved.first_style_id.is_none()
                    {
                        resolved.first_style_id = Some(item_id);
                        resolved.first_color = Some(color);
                    } else if styles.color_for_item(item_id).is_none() {
                        resolved.stats.missing_color_faces += mesh.triangle_count();
                    }
                    resolved.mesh.append_with_batch(&mesh, 0);
                }
            }
            "IFCFACEBASEDSURFACEMODEL" => {
                let color = styles
                    .color_for_item(item_id)
                    .unwrap_or([0.65, 0.65, 0.65, 1.0]);
                let item_transform =
                    if crate::geometry::item_uses_projected_coordinates(index, item_id) {
                        Mat4::identity()
                    } else {
                        transform
                    };
                let mesh = crate::geometry::mesh_face_based_surface_model(
                    index,
                    item_id,
                    &item_transform,
                    MeshBuildOptions { batch_id: 0, color },
                )?;
                if !mesh.is_empty() {
                    if styles.color_for_item(item_id).is_some() && resolved.first_style_id.is_none()
                    {
                        resolved.first_style_id = Some(item_id);
                        resolved.first_color = Some(color);
                    } else if styles.color_for_item(item_id).is_none() {
                        resolved.stats.missing_color_faces += mesh.triangle_count();
                    }
                    resolved.mesh.append_with_batch(&mesh, 0);
                }
            }
            "IFCSHELLBASEDSURFACEMODEL" => {
                let color = styles
                    .color_for_item(item_id)
                    .unwrap_or([0.65, 0.65, 0.65, 1.0]);
                let item_transform =
                    if crate::geometry::item_uses_projected_coordinates(index, item_id) {
                        Mat4::identity()
                    } else {
                        transform
                    };
                let mesh = crate::geometry::mesh_shell_based_surface_model(
                    index,
                    item_id,
                    &item_transform,
                    MeshBuildOptions { batch_id: 0, color },
                )?;
                if !mesh.is_empty() {
                    if styles.color_for_item(item_id).is_some() && resolved.first_style_id.is_none()
                    {
                        resolved.first_style_id = Some(item_id);
                        resolved.first_color = Some(color);
                    } else if styles.color_for_item(item_id).is_none() {
                        resolved.stats.missing_color_faces += mesh.triangle_count();
                    }
                    resolved.mesh.append_with_batch(&mesh, 0);
                }
            }
            "IFCEXTRUDEDAREASOLID" => {
                let color = styles
                    .color_for_item(item_id)
                    .unwrap_or([0.65, 0.65, 0.65, 1.0]);
                let item_transform =
                    if crate::geometry::item_uses_projected_coordinates(index, item_id) {
                        Mat4::identity()
                    } else {
                        transform
                    };
                let mesh = mesh_extruded_area_solid(
                    index,
                    item_id,
                    &item_transform,
                    MeshBuildOptions { batch_id: 0, color },
                )?;
                if !mesh.is_empty() {
                    if styles.color_for_item(item_id).is_some() && resolved.first_style_id.is_none()
                    {
                        resolved.first_style_id = Some(item_id);
                        resolved.first_color = Some(color);
                    } else if styles.color_for_item(item_id).is_none() {
                        resolved.stats.missing_color_faces += mesh.triangle_count();
                    }
                    resolved.mesh.append_with_batch(&mesh, 0);
                } else {
                    *resolved
                        .stats
                        .unsupported_items
                        .entry("IFCEXTRUDEDAREASOLID".to_string())
                        .or_insert(0) += 1;
                }
            }
            "IFCMAPPEDITEM" => {
                let item_args = split_arguments(index.body(item));
                let map_id = item_args
                    .first()
                    .and_then(|arg| extract_first_ref(arg))
                    .ok_or_else(|| anyhow!("mapped item #{item_id} has no source map"))?;
                let op = item_args
                    .get(1)
                    .and_then(|arg| extract_first_ref(arg))
                    .map(|id| cartesian_operator_3d(index, id))
                    .transpose()?
                    .unwrap_or_else(Mat4::identity);
                let map = index
                    .entity(map_id)
                    .ok_or_else(|| anyhow!("missing representation map #{map_id}"))?;
                let map_args = split_arguments(index.body(map));
                let origin = map_args
                    .first()
                    .and_then(|arg| extract_first_ref(arg))
                    .map(|id| axis2_placement_3d(index, id))
                    .transpose()?
                    .unwrap_or_else(Mat4::identity);
                let mapped_rep = map_args
                    .get(1)
                    .and_then(|arg| extract_first_ref(arg))
                    .ok_or_else(|| anyhow!("representation map #{map_id} has no mapped rep"))?;
                let child =
                    mesh_shape_representation(index, mapped_rep, transform * op * origin, styles)?;
                merge_resolve(&mut resolved, child);
            }
            other => {
                *resolved
                    .stats
                    .unsupported_items
                    .entry(other.to_string())
                    .or_insert(0) += 1;
            }
        }
    }
    Ok(resolved)
}

fn merge_resolve(target: &mut MeshResolve, source: MeshResolve) {
    if target.first_style_id.is_none() {
        target.first_style_id = source.first_style_id;
        target.first_color = source.first_color;
    }
    target.mesh.append_with_batch(&source.mesh, 0);
    target.stats.missing_color_faces += source.stats.missing_color_faces;
    for (key, value) in source.stats.unsupported_items {
        *target.stats.unsupported_items.entry(key).or_insert(0) += value;
    }
}

fn build_model_context(index: &StepIndex) -> ModelContext {
    let mut ctx = ModelContext::default();
    for entity in index.entities() {
        if matches!(
            entity.type_name.as_str(),
            "IFCPROJECT" | "IFCSITE" | "IFCBUILDING" | "IFCBUILDINGSTOREY" | "IFCGROUP"
        ) {
            let args = split_arguments(index.body(entity));
            ctx.names.insert(
                entity.id,
                normalize_text(
                    args.get(2)
                        .map(|arg| decode_ifc_string(arg))
                        .unwrap_or_default(),
                ),
            );
            ctx.types.insert(entity.id, entity.type_name.clone());
        }
    }

    let group_names: HashMap<u32, String> = index
        .entities_by_type("IFCGROUP")
        .filter_map(|entity| {
            ctx.names
                .get(&entity.id)
                .map(|name| (entity.id, name.clone()))
        })
        .collect();
    for rel in index.entities_by_type("IFCRELASSIGNSTOGROUP") {
        let args = split_arguments(index.body(rel));
        let related = args.get(4).map(|arg| extract_refs(arg)).unwrap_or_default();
        let Some(group_id) = args.get(6).and_then(|arg| extract_first_ref(arg)) else {
            continue;
        };
        let group_name = group_names
            .get(&group_id)
            .cloned()
            .unwrap_or_else(|| format!("#{group_id}"));
        for object_id in related {
            ctx.group_names_by_object
                .entry(object_id)
                .or_default()
                .push(group_name.clone());
        }
    }

    for rel in index.entities_by_type("IFCRELCONTAINEDINSPATIALSTRUCTURE") {
        let args = split_arguments(index.body(rel));
        let related = args.get(4).map(|arg| extract_refs(arg)).unwrap_or_default();
        let Some(container_id) = args.get(5).and_then(|arg| extract_first_ref(arg)) else {
            continue;
        };
        for object_id in related {
            ctx.contained_in.insert(object_id, container_id);
        }
    }

    for rel in index.entities_by_type("IFCRELAGGREGATES") {
        let args = split_arguments(index.body(rel));
        let Some(parent_id) = args.get(4).and_then(|arg| extract_first_ref(arg)) else {
            continue;
        };
        for child_id in args.get(5).map(|arg| extract_refs(arg)).unwrap_or_default() {
            ctx.parent_by_child.insert(child_id, parent_id);
        }
    }

    ctx.psets_by_object = build_psets(index);
    ctx
}

fn build_psets(index: &StepIndex) -> PsetsByObject {
    let mut property_values = HashMap::<u32, (String, Value)>::new();
    for entity in index.entities_by_type("IFCPROPERTYSINGLEVALUE") {
        let args = split_arguments(index.body(entity));
        let name = args
            .first()
            .map(|arg| decode_ifc_string(arg))
            .unwrap_or_default();
        let value = args
            .get(2)
            .map(|arg| parse_ifc_value(arg))
            .unwrap_or(Value::Null);
        property_values.insert(entity.id, (name, value));
    }

    let mut psets = HashMap::<u32, (String, Map<String, Value>)>::new();
    for entity in index.entities_by_type("IFCPROPERTYSET") {
        let args = split_arguments(index.body(entity));
        let pset_name = args
            .get(2)
            .map(|arg| decode_ifc_string(arg))
            .unwrap_or_default();
        let mut values = Map::new();
        for property_id in args.get(4).map(|arg| extract_refs(arg)).unwrap_or_default() {
            if let Some((name, value)) = property_values.get(&property_id) {
                values.insert(name.clone(), value.clone());
            }
        }
        psets.insert(entity.id, (pset_name, values));
    }

    let mut by_object = HashMap::<u32, Vec<(String, Map<String, Value>)>>::new();
    for rel in index.entities_by_type("IFCRELDEFINESBYPROPERTIES") {
        let args = split_arguments(index.body(rel));
        let related = args.get(4).map(|arg| extract_refs(arg)).unwrap_or_default();
        let Some(pset_id) = args.get(5).and_then(|arg| extract_first_ref(arg)) else {
            continue;
        };
        let Some(pset) = psets.get(&pset_id).cloned() else {
            continue;
        };
        for object_id in related {
            by_object.entry(object_id).or_default().push(pset.clone());
        }
    }
    by_object
}

fn collect_psets_json(ctx: &ModelContext, ids: &[Option<u32>]) -> Result<String> {
    let mut root = Map::new();
    for id in ids.iter().flatten() {
        if let Some(psets) = ctx.psets_by_object.get(id) {
            for (name, values) in psets {
                root.insert(name.clone(), Value::Object(values.clone()));
            }
        }
    }
    Ok(serde_json::to_string(&Value::Object(root))?)
}

fn parse_ifc_value(input: &str) -> Value {
    let trimmed = input.trim();
    if trimmed == "$" || trimmed == "*" {
        return Value::Null;
    }
    if trimmed.contains(".T.") {
        return Value::Bool(true);
    }
    if trimmed.contains(".F.") {
        return Value::Bool(false);
    }
    if trimmed.contains('\'')
        && let Some(start) = trimmed.find('\'')
        && let Some(end) = trimmed.rfind('\'')
    {
        return Value::String(normalize_text(decode_ifc_string(&trimmed[start..=end])));
    }
    if let Some(value) = numbers_in(trimmed).first().copied() {
        return json!(value);
    }
    Value::String(trimmed.to_string())
}

fn find_ancestor(ctx: &ModelContext, start: u32, wanted_type: &str) -> Option<u32> {
    let mut current = Some(start);
    while let Some(id) = current {
        if ctx
            .types
            .get(&id)
            .is_some_and(|type_name| type_name == wanted_type)
        {
            return Some(id);
        }
        current = ctx.parent_by_child.get(&id).copied();
    }
    None
}

fn normalize_text(input: String) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn parse_dgn_element(description: &str) -> String {
    let Some(pos) = description.find("Default:") else {
        return String::new();
    };
    description[pos + "Default:".len()..]
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect()
}

fn safe_stem(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("tileset");
    stem.chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ => ch,
        })
        .collect()
}
