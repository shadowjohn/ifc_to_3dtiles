use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::{
    b3dm, crs,
    geometry::{Bounds, Vec3},
    tiles::{self, TileJson},
};

const GLB_MAGIC: &[u8; 4] = b"glTF";
const JSON_CHUNK_TYPE: &[u8; 4] = b"JSON";
const BIN_CHUNK_TYPE: &[u8; 4] = b"BIN\0";
pub const DEFAULT_GLB_TILE_TARGET_BYTES: usize = 2_800_000;

#[derive(Debug, Clone)]
pub struct GlbToTilesOptions {
    pub input: PathBuf,
    pub output: PathBuf,
    pub longitude: f64,
    pub latitude: f64,
    pub height: f64,
    pub tile_target_bytes: usize,
    pub overwrite: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct GlbToTilesReport {
    pub input_file: String,
    pub output_dir: String,
    pub longitude: f64,
    pub latitude: f64,
    pub height: f64,
    pub byte_length: usize,
    pub tile_target_bytes: usize,
    pub bounds_min: [f64; 3],
    pub bounds_max: [f64; 3],
    pub tile_count: usize,
    pub externalized_texture_count: usize,
    pub externalized_texture_bytes: usize,
    pub tiles: Vec<GlbTileReport>,
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GlbTileReport {
    pub uri: String,
    pub primitive_count: usize,
    pub b3dm_bytes: usize,
    pub glb_bytes: usize,
    pub geometry_bytes: usize,
    pub bounds_min: [f64; 3],
    pub bounds_max: [f64; 3],
    pub over_target: bool,
}

#[derive(Debug, Clone)]
struct ParsedGlb {
    document: Value,
    bin: Vec<u8>,
}

#[derive(Debug, Clone)]
struct PrimitiveRecord {
    mesh_index: usize,
    primitive_index: usize,
    node_index: Option<usize>,
    bounds: Bounds,
    estimated_bytes: usize,
    index_start: Option<usize>,
    index_count: Option<usize>,
}

#[derive(Debug, Clone)]
struct TextureAsset {
    image_json: Value,
    byte_length: usize,
}

#[derive(Debug, Default)]
struct IdMaps {
    accessors: BTreeMap<usize, usize>,
    buffer_views: BTreeMap<usize, usize>,
    materials: BTreeMap<usize, usize>,
    textures: BTreeMap<usize, usize>,
    images: BTreeMap<usize, usize>,
    samplers: BTreeMap<usize, usize>,
}

pub fn glb_to_3dtiles(options: &GlbToTilesOptions) -> Result<PathBuf> {
    ensure!(
        options
            .input
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("glb")),
        "glb-to-3dtiles 目前只接受 .glb：{}",
        options.input.display()
    );
    ensure!(
        options.longitude.is_finite()
            && options.latitude.is_finite()
            && options.height.is_finite()
            && (-180.0..=180.0).contains(&options.longitude)
            && (-90.0..=90.0).contains(&options.latitude),
        "請提供有效 WGS84 --longitude / --latitude / --height"
    );

    let glb = fs::read(&options.input)
        .with_context(|| format!("讀取 GLB 失敗：{}", options.input.display()))?;
    let parsed = parse_glb(&glb)?;
    let root_bounds = glb_bounds_from_document(&parsed.document)?;
    let output_dir = options.output.join(safe_stem(&options.input));
    if output_dir.exists() {
        if options.overwrite {
            fs::remove_dir_all(&output_dir)
                .with_context(|| format!("清除既有輸出失敗：{}", output_dir.display()))?;
        } else {
            bail!("輸出目錄已存在，請加 --overwrite：{}", output_dir.display());
        }
    }

    let tiles_dir = output_dir.join("tiles");
    let textures_dir = tiles_dir.join("textures");
    fs::create_dir_all(&tiles_dir)
        .with_context(|| format!("建立 tiles 目錄失敗：{}", tiles_dir.display()))?;
    let texture_assets = externalize_image_buffer_views(&parsed, &textures_dir)?;
    let target_bytes = options.tile_target_bytes.max(1);
    let primitives = split_oversized_primitives(
        primitive_records(&parsed.document)?,
        &parsed,
        &parsed.document,
        target_bytes,
    )?;
    if primitives.is_empty() {
        bail!("GLB 沒有可切分的 mesh primitive");
    }

    let chunks = group_primitives(&primitives, &parsed.document, target_bytes);
    let mut children = Vec::with_capacity(chunks.len());
    let mut tile_reports = Vec::with_capacity(chunks.len());

    for (tile_index, chunk) in chunks.iter().enumerate() {
        let tile_bounds = merge_primitive_bounds(chunk);
        let tile_geometry_bytes = estimate_geometry_bytes(chunk, &parsed.document);
        let tile_glb = build_chunk_glb(&parsed, &texture_assets, chunk)?;
        let batch_table = json!({
            "source": [options.input.display().to_string()],
            "name": [safe_stem(&options.input)],
            "format": ["glb"],
            "primitive_count": [chunk.len()]
        });
        let tile_b3dm = b3dm::build_b3dm(&tile_glb, 1, &batch_table)?;
        let filename = format!("tile_{tile_index:04}.b3dm");
        fs::write(tiles_dir.join(&filename), &tile_b3dm)
            .with_context(|| format!("寫入 b3dm 失敗：{}", filename))?;
        children.push(TileJson {
            uri: format!("tiles/{filename}"),
            bounds: tile_bounds,
            geometric_error: 0.0,
        });
        tile_reports.push(GlbTileReport {
            uri: format!("tiles/{filename}"),
            primitive_count: chunk.len(),
            b3dm_bytes: tile_b3dm.len(),
            glb_bytes: tile_glb.len(),
            geometry_bytes: tile_geometry_bytes,
            bounds_min: tile_bounds.min.to_array(),
            bounds_max: tile_bounds.max.to_array(),
            over_target: tile_b3dm.len() > target_bytes,
        });
    }

    let root_transform =
        crs::enu_to_ecef_transform(options.longitude, options.latitude, options.height);
    let tileset = tiles::build_tileset_json(root_transform, &root_bounds, &children);
    fs::write(
        output_dir.join("tileset.json"),
        serde_json::to_vec_pretty(&tileset)?,
    )
    .with_context(|| format!("寫入 tileset.json 失敗：{}", output_dir.display()))?;

    let externalized_texture_bytes = texture_assets
        .iter()
        .map(|texture| texture.byte_length)
        .sum::<usize>();
    let report = GlbToTilesReport {
        input_file: options.input.display().to_string(),
        output_dir: output_dir.display().to_string(),
        longitude: options.longitude,
        latitude: options.latitude,
        height: options.height,
        byte_length: glb.len(),
        tile_target_bytes: target_bytes,
        bounds_min: root_bounds.min.to_array(),
        bounds_max: root_bounds.max.to_array(),
        tile_count: children.len(),
        externalized_texture_count: texture_assets.len(),
        externalized_texture_bytes,
        tiles: tile_reports,
        note: "GLB mesh primitives split by byte budget; oversized indexed triangle primitives are sliced by index range; embedded image bufferViews are written once under tiles/textures and referenced by tile GLBs".to_string(),
    };
    fs::write(
        output_dir.join("glb_3dtiles_report.json"),
        serde_json::to_vec_pretty(&report)?,
    )
    .with_context(|| format!("寫入 GLB report 失敗：{}", output_dir.display()))?;

    Ok(output_dir)
}

pub fn glb_bounds_from_bytes(glb: &[u8]) -> Result<Bounds> {
    let parsed = parse_glb(glb)?;
    glb_bounds_from_document(&parsed.document)
}

fn parse_glb(glb: &[u8]) -> Result<ParsedGlb> {
    ensure!(glb.len() >= 20, "GLB 檔案太短");
    ensure!(&glb[0..4] == GLB_MAGIC, "GLB magic 不正確");
    let version = u32::from_le_bytes(glb[4..8].try_into().unwrap());
    ensure!(
        version == 2,
        "目前只支援 GLB version 2，收到 version {version}"
    );
    let declared_len = u32::from_le_bytes(glb[8..12].try_into().unwrap()) as usize;
    ensure!(
        declared_len <= glb.len(),
        "GLB 宣告長度大於實際檔案大小：{declared_len} > {}",
        glb.len()
    );

    let mut offset = 12usize;
    let mut json_chunk = None;
    let mut bin = Vec::new();
    while offset + 8 <= declared_len {
        let chunk_len = u32::from_le_bytes(glb[offset..offset + 4].try_into().unwrap()) as usize;
        let chunk_type = &glb[offset + 4..offset + 8];
        let chunk_start = offset + 8;
        let chunk_end = chunk_start
            .checked_add(chunk_len)
            .context("GLB chunk 長度溢位")?;
        ensure!(chunk_end <= glb.len(), "GLB chunk 超出檔案範圍");
        if chunk_type == JSON_CHUNK_TYPE {
            json_chunk = Some(&glb[chunk_start..chunk_end]);
        } else if chunk_type == BIN_CHUNK_TYPE {
            bin.extend_from_slice(&glb[chunk_start..chunk_end]);
        }
        offset = chunk_end;
    }
    let json_chunk = json_chunk.context("GLB 缺少 JSON chunk")?;
    let document: Value = serde_json::from_slice(json_chunk).context("GLB JSON chunk 解析失敗")?;
    Ok(ParsedGlb { document, bin })
}

fn glb_bounds_from_document(document: &Value) -> Result<Bounds> {
    let mut bounds = Bounds::empty();
    let accessors = value_array(document, "accessors")?;
    for accessor_index in position_accessor_indices(document) {
        let accessor = accessors
            .get(accessor_index)
            .with_context(|| format!("POSITION accessor index 超出範圍：{accessor_index}"))?;
        let min = accessor_vec3(accessor, "min")
            .with_context(|| format!("POSITION accessor #{accessor_index} 缺少 min"))?;
        let max = accessor_vec3(accessor, "max")
            .with_context(|| format!("POSITION accessor #{accessor_index} 缺少 max"))?;
        bounds.include(min);
        bounds.include(max);
    }
    if !bounds.is_valid() {
        bail!("GLB 沒有可用 POSITION accessor min/max，無法建立 3D Tiles bounding volume");
    }
    Ok(bounds)
}

fn externalize_image_buffer_views(
    parsed: &ParsedGlb,
    textures_dir: &Path,
) -> Result<Vec<TextureAsset>> {
    let mut assets = Vec::new();
    let Some(images) = parsed.document.get("images").and_then(Value::as_array) else {
        return Ok(assets);
    };
    let empty_buffer_views = Vec::new();
    let buffer_views = parsed
        .document
        .get("bufferViews")
        .and_then(Value::as_array)
        .unwrap_or(&empty_buffer_views);
    for (index, image) in images.iter().enumerate() {
        if let Some(uri) = image.get("uri").and_then(Value::as_str) {
            assets.push(TextureAsset {
                image_json: json!({ "uri": uri }),
                byte_length: 0,
            });
            continue;
        }
        let Some(buffer_view_index) = image.get("bufferView").and_then(Value::as_u64) else {
            assets.push(TextureAsset {
                image_json: image.clone(),
                byte_length: 0,
            });
            continue;
        };
        let mime_type = image
            .get("mimeType")
            .and_then(Value::as_str)
            .unwrap_or("application/octet-stream");
        let buffer_view = buffer_views
            .get(buffer_view_index as usize)
            .with_context(|| format!("image #{index} bufferView 超出範圍"))?;
        let bytes = buffer_view_bytes(buffer_view, &parsed.bin)?;
        fs::create_dir_all(textures_dir)
            .with_context(|| format!("建立 textures 目錄失敗：{}", textures_dir.display()))?;
        let filename = format!("image_{index:04}.{}", image_extension(mime_type));
        fs::write(textures_dir.join(&filename), bytes)
            .with_context(|| format!("寫入 texture 失敗：{}", filename))?;
        let mut image_json = image.as_object().cloned().unwrap_or_default();
        image_json.remove("bufferView");
        image_json.insert(
            "uri".to_string(),
            Value::String(format!("textures/{filename}")),
        );
        assets.push(TextureAsset {
            image_json: Value::Object(image_json),
            byte_length: bytes.len(),
        });
    }
    Ok(assets)
}

fn primitive_records(document: &Value) -> Result<Vec<PrimitiveRecord>> {
    let meshes = value_array(document, "meshes")?;
    let accessors = value_array(document, "accessors")?;
    let nodes_by_mesh = nodes_by_mesh(document);
    let mut records = Vec::new();

    for (mesh_index, mesh) in meshes.iter().enumerate() {
        let Some(primitives) = mesh.get("primitives").and_then(Value::as_array) else {
            continue;
        };
        let node_indices = nodes_by_mesh
            .get(&mesh_index)
            .cloned()
            .unwrap_or_else(|| vec![None]);
        for (primitive_index, primitive) in primitives.iter().enumerate() {
            let position_index = primitive
                .get("attributes")
                .and_then(|attrs| attrs.get("POSITION"))
                .and_then(Value::as_u64)
                .context("primitive 缺少 POSITION accessor")?
                as usize;
            let position_accessor = accessors
                .get(position_index)
                .with_context(|| format!("POSITION accessor index 超出範圍：{position_index}"))?;
            let min = accessor_vec3(position_accessor, "min")
                .with_context(|| format!("POSITION accessor #{position_index} 缺少 min"))?;
            let max = accessor_vec3(position_accessor, "max")
                .with_context(|| format!("POSITION accessor #{position_index} 缺少 max"))?;
            let geometry_views = primitive_geometry_buffer_views(primitive, document);
            for node_index in &node_indices {
                let mut bounds = Bounds::empty();
                bounds.include(min);
                bounds.include(max);
                records.push(PrimitiveRecord {
                    mesh_index,
                    primitive_index,
                    node_index: *node_index,
                    bounds,
                    estimated_bytes: estimate_record_bytes(&geometry_views, document),
                    index_start: None,
                    index_count: None,
                });
            }
        }
    }
    Ok(records)
}

fn split_oversized_primitives(
    records: Vec<PrimitiveRecord>,
    parsed: &ParsedGlb,
    document: &Value,
    target_bytes: usize,
) -> Result<Vec<PrimitiveRecord>> {
    let mut split_records = Vec::with_capacity(records.len());
    for record in records {
        if record.estimated_bytes <= target_bytes {
            split_records.push(record);
            continue;
        }
        let primitive =
            &document["meshes"][record.mesh_index]["primitives"][record.primitive_index];
        let mode = primitive.get("mode").and_then(Value::as_u64).unwrap_or(4);
        if mode != 4 {
            split_records.push(record);
            continue;
        }
        let Some(indices_accessor_index) = primitive.get("indices").and_then(Value::as_u64) else {
            split_records.push(record);
            continue;
        };
        let index_accessor = document
            .get("accessors")
            .and_then(Value::as_array)
            .and_then(|accessors| accessors.get(indices_accessor_index as usize))
            .with_context(|| {
                format!("indices accessor index 超出範圍：{indices_accessor_index}")
            })?;
        let total_indices = index_accessor
            .get("count")
            .and_then(Value::as_u64)
            .context("indices accessor 缺少 count")? as usize;
        if total_indices < 6 {
            split_records.push(record);
            continue;
        }
        let slice_count = record.estimated_bytes.div_ceil(target_bytes).max(1);
        if slice_count <= 1 {
            split_records.push(record);
            continue;
        }
        let mut indices_per_slice = total_indices.div_ceil(slice_count);
        indices_per_slice = indices_per_slice.div_ceil(3).max(1) * 3;
        let mut start = 0usize;
        while start < total_indices {
            let remaining = total_indices - start;
            let mut count = remaining.min(indices_per_slice);
            if count < remaining {
                count -= count % 3;
                if count == 0 {
                    count = remaining.min(3);
                }
            }
            let mut slice = record.clone();
            slice.index_start = Some(start);
            slice.index_count = Some(count);
            slice.estimated_bytes = record.estimated_bytes.saturating_mul(count) / total_indices;
            slice.estimated_bytes = slice.estimated_bytes.max(1);
            if let Ok(bounds) = bounds_for_index_slice(parsed, primitive, start, count) {
                slice.bounds = bounds;
            }
            split_records.push(slice);
            start += count;
        }
    }
    Ok(split_records)
}

fn nodes_by_mesh(document: &Value) -> BTreeMap<usize, Vec<Option<usize>>> {
    let mut map = BTreeMap::<usize, Vec<Option<usize>>>::new();
    let Some(nodes) = document.get("nodes").and_then(Value::as_array) else {
        return map;
    };
    for (node_index, node) in nodes.iter().enumerate() {
        if let Some(mesh_index) = node.get("mesh").and_then(Value::as_u64) {
            map.entry(mesh_index as usize)
                .or_default()
                .push(Some(node_index));
        }
    }
    map
}

fn primitive_geometry_buffer_views(primitive: &Value, document: &Value) -> BTreeSet<usize> {
    let mut accessors = BTreeSet::new();
    collect_primitive_accessors(primitive, &mut accessors);
    let mut buffer_views = BTreeSet::new();
    if let Some(all_accessors) = document.get("accessors").and_then(Value::as_array) {
        for accessor_index in accessors {
            if let Some(accessor) = all_accessors.get(accessor_index) {
                collect_accessor_buffer_views(accessor, &mut buffer_views);
            }
        }
    }
    buffer_views
}

fn group_primitives(
    primitives: &[PrimitiveRecord],
    document: &Value,
    target_bytes: usize,
) -> Vec<Vec<PrimitiveRecord>> {
    let mut chunks: Vec<Vec<PrimitiveRecord>> = Vec::new();
    let mut current: Vec<PrimitiveRecord> = Vec::new();
    for primitive in primitives {
        let mut candidate = current.clone();
        candidate.push(primitive.clone());
        if !current.is_empty() && estimate_geometry_bytes(&candidate, document) > target_bytes {
            chunks.push(current);
            current = vec![primitive.clone()];
        } else {
            current = candidate;
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn estimate_record_bytes(views: &BTreeSet<usize>, document: &Value) -> usize {
    let Some(buffer_views) = document.get("bufferViews").and_then(Value::as_array) else {
        return 0;
    };
    views
        .iter()
        .filter_map(|index| buffer_views.get(*index))
        .filter_map(|view| view.get("byteLength").and_then(Value::as_u64))
        .map(|len| len as usize)
        .sum()
}

fn estimate_geometry_bytes(primitives: &[PrimitiveRecord], _document: &Value) -> usize {
    primitives
        .iter()
        .map(|primitive| primitive.estimated_bytes)
        .sum()
}

fn build_chunk_glb(
    parsed: &ParsedGlb,
    texture_assets: &[TextureAsset],
    primitives: &[PrimitiveRecord],
) -> Result<Vec<u8>> {
    let mut maps = IdMaps::default();
    let mut buffer_views = Vec::<Value>::new();
    let mut accessors = Vec::<Value>::new();
    let mut materials = Vec::<Value>::new();
    let mut textures = Vec::<Value>::new();
    let mut images = Vec::<Value>::new();
    let mut samplers = Vec::<Value>::new();
    let mut bin = Vec::<u8>::new();
    let mut mesh_values = Vec::<Value>::new();
    let mut node_values = Vec::<Value>::new();
    let mut scene_nodes = Vec::<Value>::new();

    for primitive_record in primitives {
        let primitive = parsed.document["meshes"][primitive_record.mesh_index]["primitives"]
            [primitive_record.primitive_index]
            .clone();
        let primitive = {
            let mut arrays = ChunkArrays {
                buffer_views: &mut buffer_views,
                accessors: &mut accessors,
                materials: &mut materials,
                textures: &mut textures,
                images: &mut images,
                samplers: &mut samplers,
                bin: &mut bin,
            };
            if primitive_record.index_start.is_some() {
                remap_indexed_primitive_slice(
                    primitive,
                    primitive_record,
                    parsed,
                    texture_assets,
                    &mut maps,
                    &mut arrays,
                )?
            } else {
                remap_primitive(primitive, parsed, texture_assets, &mut maps, &mut arrays)?
            }
        };
        let mesh_index = mesh_values.len();
        mesh_values.push(json!({ "primitives": [primitive] }));
        let node_index = node_values.len();
        let mut node = primitive_record
            .node_index
            .and_then(|index| parsed.document["nodes"].get(index).cloned())
            .unwrap_or_else(|| json!({}));
        if let Some(object) = node.as_object_mut() {
            object.insert("mesh".to_string(), json!(mesh_index));
            object.remove("children");
            object.remove("skin");
            object.remove("camera");
        } else {
            node = json!({ "mesh": mesh_index });
        }
        node_values.push(node);
        scene_nodes.push(json!(node_index));
    }

    let mut document = Map::new();
    document.insert(
        "asset".to_string(),
        parsed
            .document
            .get("asset")
            .cloned()
            .unwrap_or_else(|| json!({ "version": "2.0" })),
    );
    document.insert("scene".to_string(), json!(0));
    document.insert("scenes".to_string(), json!([{ "nodes": scene_nodes }]));
    document.insert("nodes".to_string(), Value::Array(node_values));
    document.insert("meshes".to_string(), Value::Array(mesh_values));
    document.insert("accessors".to_string(), Value::Array(accessors));
    document.insert("bufferViews".to_string(), Value::Array(buffer_views));
    for key in ["extensionsUsed", "extensionsRequired"] {
        if let Some(value) = parsed.document.get(key) {
            document.insert(key.to_string(), value.clone());
        }
    }
    if !bin.is_empty() {
        document.insert("buffers".to_string(), json!([{ "byteLength": bin.len() }]));
    }
    if !materials.is_empty() {
        document.insert("materials".to_string(), Value::Array(materials));
    }
    if !textures.is_empty() {
        document.insert("textures".to_string(), Value::Array(textures));
    }
    if !images.is_empty() {
        document.insert("images".to_string(), Value::Array(images));
    }
    if !samplers.is_empty() {
        document.insert("samplers".to_string(), Value::Array(samplers));
    }
    Ok(build_glb(Value::Object(document), bin))
}

struct ChunkArrays<'a> {
    buffer_views: &'a mut Vec<Value>,
    accessors: &'a mut Vec<Value>,
    materials: &'a mut Vec<Value>,
    textures: &'a mut Vec<Value>,
    images: &'a mut Vec<Value>,
    samplers: &'a mut Vec<Value>,
    bin: &'a mut Vec<u8>,
}

fn remap_primitive(
    mut primitive: Value,
    parsed: &ParsedGlb,
    texture_assets: &[TextureAsset],
    maps: &mut IdMaps,
    arrays: &mut ChunkArrays<'_>,
) -> Result<Value> {
    if let Some(attributes) = primitive
        .get_mut("attributes")
        .and_then(Value::as_object_mut)
    {
        for value in attributes.values_mut() {
            if let Some(index) = value.as_u64() {
                *value = json!(copy_accessor(index as usize, parsed, maps, arrays)?);
            }
        }
    }
    if let Some(index) = primitive.get("indices").and_then(Value::as_u64) {
        primitive["indices"] = json!(copy_accessor(index as usize, parsed, maps, arrays)?);
    }
    if let Some(targets) = primitive.get_mut("targets").and_then(Value::as_array_mut) {
        for target in targets {
            if let Some(target_object) = target.as_object_mut() {
                for value in target_object.values_mut() {
                    if let Some(index) = value.as_u64() {
                        *value = json!(copy_accessor(index as usize, parsed, maps, arrays)?);
                    }
                }
            }
        }
    }
    if let Some(material_index) = primitive.get("material").and_then(Value::as_u64) {
        primitive["material"] = json!(copy_material(
            material_index as usize,
            parsed,
            texture_assets,
            maps,
            arrays,
        )?);
    }
    Ok(primitive)
}

fn remap_indexed_primitive_slice(
    mut primitive: Value,
    record: &PrimitiveRecord,
    parsed: &ParsedGlb,
    texture_assets: &[TextureAsset],
    maps: &mut IdMaps,
    arrays: &mut ChunkArrays<'_>,
) -> Result<Value> {
    let start = record
        .index_start
        .context("indexed primitive slice 缺少 start")?;
    let count = record
        .index_count
        .context("indexed primitive slice 缺少 count")?;
    let source_indices_accessor = primitive
        .get("indices")
        .and_then(Value::as_u64)
        .context("indexed primitive slice 缺少 indices accessor")?
        as usize;
    let source_indices = read_indices(parsed, source_indices_accessor, start, count)?;

    let mut vertex_map = HashMap::<u32, u32>::new();
    let mut ordered_vertices = Vec::<usize>::new();
    let mut remapped_indices = Vec::<u32>::with_capacity(source_indices.len());
    for source_index in source_indices {
        let next_index = vertex_map.len() as u32;
        let mapped = *vertex_map.entry(source_index).or_insert_with(|| {
            ordered_vertices.push(source_index as usize);
            next_index
        });
        remapped_indices.push(mapped);
    }

    if let Some(attributes) = primitive
        .get_mut("attributes")
        .and_then(Value::as_object_mut)
    {
        let keys = attributes.keys().cloned().collect::<Vec<_>>();
        for key in keys {
            let Some(index) = attributes.get(&key).and_then(Value::as_u64) else {
                continue;
            };
            let mapped = copy_accessor_elements(
                index as usize,
                &ordered_vertices,
                parsed,
                arrays,
                key == "POSITION",
            )?;
            attributes.insert(key, json!(mapped));
        }
    }
    primitive["indices"] = json!(write_indices_accessor(&remapped_indices, arrays)?);
    if let Some(targets) = primitive.get_mut("targets").and_then(Value::as_array_mut) {
        for target in targets {
            if let Some(target_object) = target.as_object_mut() {
                let keys = target_object.keys().cloned().collect::<Vec<_>>();
                for key in keys {
                    let Some(index) = target_object.get(&key).and_then(Value::as_u64) else {
                        continue;
                    };
                    let mapped = copy_accessor_elements(
                        index as usize,
                        &ordered_vertices,
                        parsed,
                        arrays,
                        false,
                    )?;
                    target_object.insert(key, json!(mapped));
                }
            }
        }
    }
    if let Some(material_index) = primitive.get("material").and_then(Value::as_u64) {
        primitive["material"] = json!(copy_material(
            material_index as usize,
            parsed,
            texture_assets,
            maps,
            arrays,
        )?);
    }
    Ok(primitive)
}

fn copy_accessor_elements(
    index: usize,
    elements: &[usize],
    parsed: &ParsedGlb,
    arrays: &mut ChunkArrays<'_>,
    recompute_bounds: bool,
) -> Result<usize> {
    let accessors = value_array(&parsed.document, "accessors")?;
    let accessor = accessors
        .get(index)
        .with_context(|| format!("accessor index 超出範圍：{index}"))?;
    ensure!(
        accessor.get("sparse").is_none(),
        "GLB sparse accessor 暫不支援分片"
    );
    let element_size = accessor_element_size(accessor)?;
    let source_count = accessor
        .get("count")
        .and_then(Value::as_u64)
        .context("accessor 缺少 count")? as usize;
    let mut bytes = Vec::<u8>::with_capacity(elements.len() * element_size);

    if let Some(buffer_view_index) = accessor.get("bufferView").and_then(Value::as_u64) {
        let buffer_views = value_array(&parsed.document, "bufferViews")?;
        let buffer_view = buffer_views
            .get(buffer_view_index as usize)
            .with_context(|| format!("bufferView index 超出範圍：{buffer_view_index}"))?;
        let view_bytes = buffer_view_bytes(buffer_view, &parsed.bin)?;
        let accessor_offset = accessor
            .get("byteOffset")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize;
        let stride = buffer_view
            .get("byteStride")
            .and_then(Value::as_u64)
            .map(|stride| stride as usize)
            .unwrap_or(element_size);
        for element in elements {
            ensure!(
                *element < source_count,
                "accessor element index 超出範圍：{} >= {}",
                element,
                source_count
            );
            let start = accessor_offset
                .checked_add(
                    element
                        .checked_mul(stride)
                        .context("accessor stride 溢位")?,
                )
                .context("accessor byteOffset 溢位")?;
            let end = start
                .checked_add(element_size)
                .context("accessor element 溢位")?;
            ensure!(
                end <= view_bytes.len(),
                "accessor element 超出 bufferView 範圍"
            );
            bytes.extend_from_slice(&view_bytes[start..end]);
        }
    } else {
        for element in elements {
            ensure!(
                *element < source_count,
                "accessor element index 超出範圍：{} >= {}",
                element,
                source_count
            );
        }
        bytes.resize(elements.len() * element_size, 0);
    }

    let target = accessor
        .get("bufferView")
        .and_then(Value::as_u64)
        .and_then(|buffer_view_index| {
            parsed
                .document
                .get("bufferViews")
                .and_then(Value::as_array)
                .and_then(|views| views.get(buffer_view_index as usize))
                .and_then(|view| view.get("target"))
                .and_then(Value::as_u64)
        });
    let buffer_view = append_buffer_bytes(arrays, &bytes, target);
    let mut accessor_json = accessor.as_object().cloned().unwrap_or_default();
    accessor_json.remove("sparse");
    accessor_json.insert("bufferView".to_string(), json!(buffer_view));
    accessor_json.insert("byteOffset".to_string(), json!(0));
    accessor_json.insert("count".to_string(), json!(elements.len()));
    accessor_json.remove("min");
    accessor_json.remove("max");
    if recompute_bounds {
        if let Some(bounds) = bounds_from_accessor_bytes(&bytes, accessor)? {
            accessor_json.insert("min".to_string(), json!(bounds.min.to_array()));
            accessor_json.insert("max".to_string(), json!(bounds.max.to_array()));
        }
    }
    let mapped = arrays.accessors.len();
    arrays.accessors.push(Value::Object(accessor_json));
    Ok(mapped)
}

fn write_indices_accessor(indices: &[u32], arrays: &mut ChunkArrays<'_>) -> Result<usize> {
    let max_index = indices.iter().copied().max().unwrap_or(0);
    let component_type = if max_index <= u16::MAX as u32 {
        5123u32
    } else {
        5125u32
    };
    let mut bytes = Vec::<u8>::new();
    if component_type == 5123 {
        bytes.reserve(indices.len() * 2);
        for index in indices {
            bytes.extend_from_slice(&(*index as u16).to_le_bytes());
        }
    } else {
        bytes.reserve(indices.len() * 4);
        for index in indices {
            bytes.extend_from_slice(&index.to_le_bytes());
        }
    }
    let buffer_view = append_buffer_bytes(arrays, &bytes, Some(34963));
    let accessor = json!({
        "bufferView": buffer_view,
        "byteOffset": 0,
        "componentType": component_type,
        "count": indices.len(),
        "type": "SCALAR",
        "min": [0],
        "max": [max_index]
    });
    let mapped = arrays.accessors.len();
    arrays.accessors.push(accessor);
    Ok(mapped)
}

fn copy_accessor(
    index: usize,
    parsed: &ParsedGlb,
    maps: &mut IdMaps,
    arrays: &mut ChunkArrays<'_>,
) -> Result<usize> {
    if let Some(mapped) = maps.accessors.get(&index) {
        return Ok(*mapped);
    }
    let mut accessor = parsed.document["accessors"][index].clone();
    if let Some(buffer_view) = accessor.get("bufferView").and_then(Value::as_u64) {
        accessor["bufferView"] = json!(copy_buffer_view(
            buffer_view as usize,
            parsed,
            maps,
            arrays,
        )?);
    }
    if let Some(sparse) = accessor.get_mut("sparse") {
        for key in ["indices", "values"] {
            if let Some(buffer_view) = sparse
                .get(key)
                .and_then(|value| value.get("bufferView"))
                .and_then(Value::as_u64)
            {
                sparse[key]["bufferView"] = json!(copy_buffer_view(
                    buffer_view as usize,
                    parsed,
                    maps,
                    arrays
                )?);
            }
        }
    }
    let mapped = arrays.accessors.len();
    arrays.accessors.push(accessor);
    maps.accessors.insert(index, mapped);
    Ok(mapped)
}

fn copy_buffer_view(
    index: usize,
    parsed: &ParsedGlb,
    maps: &mut IdMaps,
    arrays: &mut ChunkArrays<'_>,
) -> Result<usize> {
    if let Some(mapped) = maps.buffer_views.get(&index) {
        return Ok(*mapped);
    }
    while !arrays.bin.len().is_multiple_of(4) {
        arrays.bin.push(0);
    }
    let source_view = parsed
        .document
        .get("bufferViews")
        .and_then(Value::as_array)
        .and_then(|views| views.get(index))
        .with_context(|| format!("bufferView index 超出範圍：{index}"))?;
    let source_bytes = buffer_view_bytes(source_view, &parsed.bin)?;
    let byte_offset = arrays.bin.len();
    arrays.bin.extend_from_slice(source_bytes);
    let mut view = source_view.as_object().cloned().unwrap_or_default();
    view.insert("buffer".to_string(), json!(0));
    view.insert("byteOffset".to_string(), json!(byte_offset));
    view.insert("byteLength".to_string(), json!(source_bytes.len()));
    let mapped = arrays.buffer_views.len();
    arrays.buffer_views.push(Value::Object(view));
    maps.buffer_views.insert(index, mapped);
    Ok(mapped)
}

fn copy_material(
    index: usize,
    parsed: &ParsedGlb,
    texture_assets: &[TextureAsset],
    maps: &mut IdMaps,
    arrays: &mut ChunkArrays<'_>,
) -> Result<usize> {
    if let Some(mapped) = maps.materials.get(&index) {
        return Ok(*mapped);
    }
    let mut material = parsed.document["materials"][index].clone();
    remap_texture_indices(&mut material, parsed, texture_assets, maps, arrays)?;
    let mapped = arrays.materials.len();
    arrays.materials.push(material);
    maps.materials.insert(index, mapped);
    Ok(mapped)
}

fn remap_texture_indices(
    value: &mut Value,
    parsed: &ParsedGlb,
    texture_assets: &[TextureAsset],
    maps: &mut IdMaps,
    arrays: &mut ChunkArrays<'_>,
) -> Result<()> {
    match value {
        Value::Object(object) => {
            if let Some(index) = object.get("index").and_then(Value::as_u64) {
                object.insert(
                    "index".to_string(),
                    json!(copy_texture(
                        index as usize,
                        parsed,
                        texture_assets,
                        maps,
                        arrays
                    )?),
                );
            }
            for nested in object.values_mut() {
                remap_texture_indices(nested, parsed, texture_assets, maps, arrays)?;
            }
        }
        Value::Array(values) => {
            for nested in values {
                remap_texture_indices(nested, parsed, texture_assets, maps, arrays)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn copy_texture(
    index: usize,
    parsed: &ParsedGlb,
    texture_assets: &[TextureAsset],
    maps: &mut IdMaps,
    arrays: &mut ChunkArrays<'_>,
) -> Result<usize> {
    if let Some(mapped) = maps.textures.get(&index) {
        return Ok(*mapped);
    }
    let mut texture = parsed.document["textures"][index].clone();
    if let Some(source) = texture.get("source").and_then(Value::as_u64) {
        texture["source"] = json!(copy_image(source as usize, texture_assets, maps, arrays)?);
    }
    if let Some(sampler) = texture.get("sampler").and_then(Value::as_u64) {
        texture["sampler"] = json!(copy_sampler(sampler as usize, parsed, maps, arrays)?);
    }
    let mapped = arrays.textures.len();
    arrays.textures.push(texture);
    maps.textures.insert(index, mapped);
    Ok(mapped)
}

fn copy_image(
    index: usize,
    texture_assets: &[TextureAsset],
    maps: &mut IdMaps,
    arrays: &mut ChunkArrays<'_>,
) -> Result<usize> {
    if let Some(mapped) = maps.images.get(&index) {
        return Ok(*mapped);
    }
    let image = texture_assets
        .get(index)
        .with_context(|| format!("image index 超出範圍：{index}"))?
        .image_json
        .clone();
    let mapped = arrays.images.len();
    arrays.images.push(image);
    maps.images.insert(index, mapped);
    Ok(mapped)
}

fn copy_sampler(
    index: usize,
    parsed: &ParsedGlb,
    maps: &mut IdMaps,
    arrays: &mut ChunkArrays<'_>,
) -> Result<usize> {
    if let Some(mapped) = maps.samplers.get(&index) {
        return Ok(*mapped);
    }
    let sampler = parsed.document["samplers"][index].clone();
    let mapped = arrays.samplers.len();
    arrays.samplers.push(sampler);
    maps.samplers.insert(index, mapped);
    Ok(mapped)
}

fn collect_primitive_accessors(primitive: &Value, accessors: &mut BTreeSet<usize>) {
    if let Some(attributes) = primitive.get("attributes").and_then(Value::as_object) {
        for value in attributes.values() {
            if let Some(index) = value.as_u64() {
                accessors.insert(index as usize);
            }
        }
    }
    if let Some(index) = primitive.get("indices").and_then(Value::as_u64) {
        accessors.insert(index as usize);
    }
    if let Some(targets) = primitive.get("targets").and_then(Value::as_array) {
        for target in targets {
            if let Some(target_object) = target.as_object() {
                for value in target_object.values() {
                    if let Some(index) = value.as_u64() {
                        accessors.insert(index as usize);
                    }
                }
            }
        }
    }
}

fn collect_accessor_buffer_views(accessor: &Value, buffer_views: &mut BTreeSet<usize>) {
    if let Some(buffer_view) = accessor.get("bufferView").and_then(Value::as_u64) {
        buffer_views.insert(buffer_view as usize);
    }
    if let Some(sparse) = accessor.get("sparse") {
        for key in ["indices", "values"] {
            if let Some(buffer_view) = sparse
                .get(key)
                .and_then(|value| value.get("bufferView"))
                .and_then(Value::as_u64)
            {
                buffer_views.insert(buffer_view as usize);
            }
        }
    }
}

fn merge_primitive_bounds(primitives: &[PrimitiveRecord]) -> Bounds {
    let mut bounds = Bounds::empty();
    for primitive in primitives {
        bounds.include_bounds(&primitive.bounds);
    }
    bounds
}

fn buffer_view_bytes<'a>(buffer_view: &Value, bin: &'a [u8]) -> Result<&'a [u8]> {
    let offset = buffer_view
        .get("byteOffset")
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    let len = buffer_view
        .get("byteLength")
        .and_then(Value::as_u64)
        .context("bufferView 缺少 byteLength")? as usize;
    let end = offset.checked_add(len).context("bufferView 長度溢位")?;
    ensure!(end <= bin.len(), "bufferView 超出 BIN chunk 範圍");
    Ok(&bin[offset..end])
}

fn append_buffer_bytes(
    arrays: &mut ChunkArrays<'_>,
    source_bytes: &[u8],
    target: Option<u64>,
) -> usize {
    while !arrays.bin.len().is_multiple_of(4) {
        arrays.bin.push(0);
    }
    let byte_offset = arrays.bin.len();
    arrays.bin.extend_from_slice(source_bytes);
    let mut view = Map::new();
    view.insert("buffer".to_string(), json!(0));
    view.insert("byteOffset".to_string(), json!(byte_offset));
    view.insert("byteLength".to_string(), json!(source_bytes.len()));
    if let Some(target) = target {
        view.insert("target".to_string(), json!(target));
    }
    let mapped = arrays.buffer_views.len();
    arrays.buffer_views.push(Value::Object(view));
    mapped
}

fn read_indices(
    parsed: &ParsedGlb,
    accessor_index: usize,
    start: usize,
    count: usize,
) -> Result<Vec<u32>> {
    let accessors = value_array(&parsed.document, "accessors")?;
    let accessor = accessors
        .get(accessor_index)
        .with_context(|| format!("indices accessor index 超出範圍：{accessor_index}"))?;
    ensure!(
        accessor.get("sparse").is_none(),
        "GLB sparse indices 暫不支援分片"
    );
    let component_type = accessor
        .get("componentType")
        .and_then(Value::as_u64)
        .context("indices accessor 缺少 componentType")? as u32;
    ensure!(
        matches!(component_type, 5121 | 5123 | 5125),
        "indices accessor componentType 不支援：{}",
        component_type
    );
    ensure!(
        accessor
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("SCALAR")
            == "SCALAR",
        "indices accessor type 必須是 SCALAR"
    );
    let source_count = accessor
        .get("count")
        .and_then(Value::as_u64)
        .context("indices accessor 缺少 count")? as usize;
    ensure!(
        start.checked_add(count).context("indices range 溢位")? <= source_count,
        "indices slice 超出 accessor 範圍"
    );
    let buffer_view_index = accessor
        .get("bufferView")
        .and_then(Value::as_u64)
        .context("indices accessor 缺少 bufferView")? as usize;
    let buffer_views = value_array(&parsed.document, "bufferViews")?;
    let buffer_view = buffer_views
        .get(buffer_view_index)
        .with_context(|| format!("indices bufferView index 超出範圍：{buffer_view_index}"))?;
    let view_bytes = buffer_view_bytes(buffer_view, &parsed.bin)?;
    let element_size = component_size(component_type)?;
    let accessor_offset = accessor
        .get("byteOffset")
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    let stride = buffer_view
        .get("byteStride")
        .and_then(Value::as_u64)
        .map(|stride| stride as usize)
        .unwrap_or(element_size);
    let mut indices = Vec::with_capacity(count);
    for element in start..start + count {
        let byte_start = accessor_offset
            .checked_add(element.checked_mul(stride).context("indices stride 溢位")?)
            .context("indices byteOffset 溢位")?;
        let byte_end = byte_start
            .checked_add(element_size)
            .context("indices element 溢位")?;
        ensure!(
            byte_end <= view_bytes.len(),
            "indices element 超出 bufferView 範圍"
        );
        let index = match component_type {
            5121 => view_bytes[byte_start] as u32,
            5123 => u16::from_le_bytes(view_bytes[byte_start..byte_end].try_into().unwrap()) as u32,
            5125 => u32::from_le_bytes(view_bytes[byte_start..byte_end].try_into().unwrap()),
            _ => unreachable!(),
        };
        indices.push(index);
    }
    Ok(indices)
}

fn bounds_for_index_slice(
    parsed: &ParsedGlb,
    primitive: &Value,
    start: usize,
    count: usize,
) -> Result<Bounds> {
    let indices_accessor = primitive
        .get("indices")
        .and_then(Value::as_u64)
        .context("primitive 缺少 indices accessor")? as usize;
    let position_accessor = primitive
        .get("attributes")
        .and_then(|attributes| attributes.get("POSITION"))
        .and_then(Value::as_u64)
        .context("primitive 缺少 POSITION accessor")? as usize;
    let mut bounds = Bounds::empty();
    for index in read_indices(parsed, indices_accessor, start, count)? {
        bounds.include(read_accessor_vec3(
            parsed,
            position_accessor,
            index as usize,
        )?);
    }
    ensure!(bounds.is_valid(), "indices slice 無有效 bounds");
    Ok(bounds)
}

fn read_accessor_vec3(parsed: &ParsedGlb, accessor_index: usize, element: usize) -> Result<Vec3> {
    let accessors = value_array(&parsed.document, "accessors")?;
    let accessor = accessors
        .get(accessor_index)
        .with_context(|| format!("POSITION accessor index 超出範圍：{accessor_index}"))?;
    ensure!(
        accessor.get("componentType").and_then(Value::as_u64) == Some(5126),
        "POSITION accessor componentType 必須是 FLOAT"
    );
    ensure!(
        accessor.get("type").and_then(Value::as_str) == Some("VEC3"),
        "POSITION accessor type 必須是 VEC3"
    );
    let count = accessor
        .get("count")
        .and_then(Value::as_u64)
        .context("POSITION accessor 缺少 count")? as usize;
    ensure!(element < count, "POSITION element index 超出範圍");
    let buffer_view_index = accessor
        .get("bufferView")
        .and_then(Value::as_u64)
        .context("POSITION accessor 缺少 bufferView")? as usize;
    let buffer_views = value_array(&parsed.document, "bufferViews")?;
    let buffer_view = buffer_views
        .get(buffer_view_index)
        .with_context(|| format!("POSITION bufferView index 超出範圍：{buffer_view_index}"))?;
    let view_bytes = buffer_view_bytes(buffer_view, &parsed.bin)?;
    let element_size = accessor_element_size(accessor)?;
    let accessor_offset = accessor
        .get("byteOffset")
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    let stride = buffer_view
        .get("byteStride")
        .and_then(Value::as_u64)
        .map(|stride| stride as usize)
        .unwrap_or(element_size);
    let start = accessor_offset
        .checked_add(
            element
                .checked_mul(stride)
                .context("POSITION stride 溢位")?,
        )
        .context("POSITION byteOffset 溢位")?;
    read_vec3_from_bytes(view_bytes, start)
}

fn bounds_from_accessor_bytes(bytes: &[u8], accessor: &Value) -> Result<Option<Bounds>> {
    if accessor.get("componentType").and_then(Value::as_u64) != Some(5126)
        || accessor.get("type").and_then(Value::as_str) != Some("VEC3")
    {
        return Ok(None);
    }
    let mut bounds = Bounds::empty();
    for chunk in bytes.chunks_exact(12) {
        bounds.include(read_vec3_from_bytes(chunk, 0)?);
    }
    if bounds.is_valid() {
        Ok(Some(bounds))
    } else {
        Ok(None)
    }
}

fn read_vec3_from_bytes(bytes: &[u8], start: usize) -> Result<Vec3> {
    let end = start.checked_add(12).context("VEC3 byte range 溢位")?;
    ensure!(end <= bytes.len(), "VEC3 超出 buffer 範圍");
    let x = f32::from_le_bytes(bytes[start..start + 4].try_into().unwrap()) as f64;
    let y = f32::from_le_bytes(bytes[start + 4..start + 8].try_into().unwrap()) as f64;
    let z = f32::from_le_bytes(bytes[start + 8..start + 12].try_into().unwrap()) as f64;
    Ok(Vec3::new(x, y, z))
}

fn accessor_element_size(accessor: &Value) -> Result<usize> {
    let component_type = accessor
        .get("componentType")
        .and_then(Value::as_u64)
        .context("accessor 缺少 componentType")? as u32;
    let accessor_type = accessor
        .get("type")
        .and_then(Value::as_str)
        .context("accessor 缺少 type")?;
    Ok(component_size(component_type)? * accessor_type_components(accessor_type)?)
}

fn component_size(component_type: u32) -> Result<usize> {
    match component_type {
        5120 | 5121 => Ok(1),
        5122 | 5123 => Ok(2),
        5125 | 5126 => Ok(4),
        _ => bail!("accessor componentType 不支援：{component_type}"),
    }
}

fn accessor_type_components(accessor_type: &str) -> Result<usize> {
    match accessor_type {
        "SCALAR" => Ok(1),
        "VEC2" => Ok(2),
        "VEC3" => Ok(3),
        "VEC4" => Ok(4),
        "MAT2" => Ok(4),
        "MAT3" => Ok(9),
        "MAT4" => Ok(16),
        _ => bail!("accessor type 不支援：{accessor_type}"),
    }
}

fn position_accessor_indices(document: &Value) -> Vec<usize> {
    let Some(meshes) = document.get("meshes").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut indices = Vec::new();
    for mesh in meshes {
        let Some(primitives) = mesh.get("primitives").and_then(Value::as_array) else {
            continue;
        };
        for primitive in primitives {
            let Some(index) = primitive
                .get("attributes")
                .and_then(|attrs| attrs.get("POSITION"))
                .and_then(Value::as_u64)
            else {
                continue;
            };
            indices.push(index as usize);
        }
    }
    indices
}

fn accessor_vec3(accessor: &Value, name: &str) -> Option<Vec3> {
    let values = accessor.get(name)?.as_array()?;
    if values.len() < 3 {
        return None;
    }
    Some(Vec3::new(
        values.first()?.as_f64()?,
        values.get(1)?.as_f64()?,
        values.get(2)?.as_f64()?,
    ))
}

fn value_array<'a>(value: &'a Value, key: &str) -> Result<&'a Vec<Value>> {
    value
        .get(key)
        .and_then(Value::as_array)
        .with_context(|| format!("GLB 缺少 {key}"))
}

fn image_extension(mime_type: &str) -> &'static str {
    match mime_type {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/webp" => "webp",
        _ => "bin",
    }
}

fn build_glb(document: Value, mut bin: Vec<u8>) -> Vec<u8> {
    let mut json_bytes = serde_json::to_vec(&document).expect("serialize glb json");
    while !json_bytes.len().is_multiple_of(4) {
        json_bytes.push(b' ');
    }
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let bin_chunk_len = if bin.is_empty() { 0 } else { 8 + bin.len() };
    let total_len = 12 + 8 + json_bytes.len() + bin_chunk_len;
    let mut glb = Vec::with_capacity(total_len);
    glb.extend_from_slice(GLB_MAGIC);
    glb.extend_from_slice(&2u32.to_le_bytes());
    glb.extend_from_slice(&(total_len as u32).to_le_bytes());
    glb.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    glb.extend_from_slice(JSON_CHUNK_TYPE);
    glb.extend_from_slice(&json_bytes);
    if !bin.is_empty() {
        glb.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        glb.extend_from_slice(BIN_CHUNK_TYPE);
        glb.extend_from_slice(&bin);
    }
    glb
}

fn safe_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("tileset")
        .to_string()
}
