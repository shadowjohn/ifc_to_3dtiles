use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::Serialize;

use crate::step::{StepIndex, decode_ifc_string, extract_first_ref, extract_refs, split_arguments};

#[derive(Debug, Clone)]
pub struct ConvertedProductInfo {
    pub ifc_step_id: u32,
    pub triangle_count: usize,
    pub wgs84_bounds_min: Option<Wgs84Coordinate>,
    pub wgs84_bounds_max: Option<Wgs84Coordinate>,
    pub wgs84_center: Option<Wgs84Coordinate>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Wgs84Coordinate {
    pub lon: f64,
    pub lat: f64,
    pub height: f64,
}

impl Wgs84Coordinate {
    pub fn new(lon: f64, lat: f64, height: f64) -> Self {
        Self { lon, lat, height }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct IfcCoordinateInfo {
    pub source_epsg: Option<u32>,
    pub wgs84_bounds_min: Option<Wgs84Coordinate>,
    pub wgs84_bounds_max: Option<Wgs84Coordinate>,
    pub wgs84_center: Option<Wgs84Coordinate>,
    pub wgs84_origin: Option<Wgs84Coordinate>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IfcInfoReport {
    pub input_file: String,
    pub entity_count: usize,
    pub entity_type_counts: BTreeMap<String, usize>,
    pub coordinate_info: IfcCoordinateInfo,
    pub product_count: usize,
    pub converted_product_count: usize,
    pub skipped_product_count: usize,
    pub property_count: usize,
    pub geometry_item_count: usize,
    pub products: Vec<IfcProductInfo>,
    pub properties: Vec<IfcPropertyInfo>,
    pub geometry_items: Vec<IfcGeometryItemInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IfcProductInfo {
    pub ifc_step_id: u32,
    pub global_id: String,
    pub ifc_type: String,
    pub name: String,
    pub description: String,
    pub object_type: String,
    pub tag: String,
    pub representation_step_id: u32,
    pub converted: bool,
    pub triangle_count: usize,
    pub wgs84_bounds_min: Option<Wgs84Coordinate>,
    pub wgs84_bounds_max: Option<Wgs84Coordinate>,
    pub wgs84_center: Option<Wgs84Coordinate>,
    pub property_count: usize,
    pub geometry_item_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct IfcPropertyInfo {
    pub ifc_step_id: u32,
    pub global_id: String,
    pub ifc_type: String,
    pub name: String,
    pub pset_name: String,
    pub property_name: String,
    pub property_value: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IfcGeometryItemInfo {
    pub ifc_step_id: u32,
    pub global_id: String,
    pub ifc_type: String,
    pub name: String,
    pub representation_step_id: u32,
    pub representation_identifier: String,
    pub representation_type: String,
    pub item_step_id: u32,
    pub item_type: String,
    pub resolved_item_type: String,
    pub supported: bool,
}

#[derive(Debug, Clone)]
struct ProductCore {
    ifc_step_id: u32,
    global_id: String,
    ifc_type: String,
    name: String,
    description: String,
    object_type: String,
    tag: String,
    representation_step_id: u32,
}

#[derive(Debug, Clone)]
struct PropertyValue {
    pset_name: String,
    property_name: String,
    property_value: String,
}

pub fn write_ifc_info_path(input: &Path, output: &Path) -> Result<Vec<PathBuf>> {
    let mut inputs = Vec::new();
    if input.is_file() {
        inputs.push(input.to_path_buf());
    } else {
        for entry in fs::read_dir(input)
            .with_context(|| format!("讀取 IFC info 輸入目錄失敗：{}", input.display()))?
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
        bail!("找不到 IFC 檔案：{}", input.display());
    }

    let mut outputs = Vec::new();
    if input.is_file() {
        fs::create_dir_all(output)
            .with_context(|| format!("建立 IFC info 輸出目錄失敗：{}", output.display()))?;
        write_ifc_info_file(&inputs[0], output, &[])?;
        outputs.push(output.to_path_buf());
    } else {
        fs::create_dir_all(output)
            .with_context(|| format!("建立 IFC info 輸出目錄失敗：{}", output.display()))?;
        for input_file in inputs {
            let output_dir = output.join(safe_stem(&input_file));
            fs::create_dir_all(&output_dir)
                .with_context(|| format!("建立 IFC info 輸出目錄失敗：{}", output_dir.display()))?;
            write_ifc_info_file(&input_file, &output_dir, &[])?;
            outputs.push(output_dir);
        }
    }
    Ok(outputs)
}

pub fn write_ifc_info_file(
    input_file: &Path,
    output_dir: &Path,
    converted_products: &[ConvertedProductInfo],
) -> Result<IfcInfoReport> {
    let content = fs::read_to_string(input_file)
        .with_context(|| format!("讀取 IFC info 失敗：{}", input_file.display()))?;
    let index = StepIndex::parse(content);
    write_ifc_info_outputs(input_file, output_dir, &index, converted_products)
}

pub fn write_ifc_info_outputs(
    input_file: &Path,
    output_dir: &Path,
    index: &StepIndex,
    converted_products: &[ConvertedProductInfo],
) -> Result<IfcInfoReport> {
    write_ifc_info_outputs_with_coordinate_info(
        input_file,
        output_dir,
        index,
        converted_products,
        None,
    )
}

pub fn write_ifc_info_outputs_with_coordinate_info(
    input_file: &Path,
    output_dir: &Path,
    index: &StepIndex,
    converted_products: &[ConvertedProductInfo],
    coordinate_info: Option<IfcCoordinateInfo>,
) -> Result<IfcInfoReport> {
    fs::create_dir_all(output_dir)
        .with_context(|| format!("建立 IFC info 輸出目錄失敗：{}", output_dir.display()))?;
    let report = build_ifc_info_report(input_file, index, converted_products, coordinate_info);
    fs::write(
        output_dir.join("ifc_info.json"),
        serde_json::to_vec_pretty(&report)?,
    )
    .with_context(|| format!("寫入 ifc_info.json 失敗：{}", output_dir.display()))?;
    fs::write(
        output_dir.join("ifc_products.csv"),
        render_products_csv(&report.products),
    )
    .with_context(|| format!("寫入 ifc_products.csv 失敗：{}", output_dir.display()))?;
    fs::write(
        output_dir.join("ifc_properties.csv"),
        render_properties_csv(&report.properties),
    )
    .with_context(|| format!("寫入 ifc_properties.csv 失敗：{}", output_dir.display()))?;
    fs::write(
        output_dir.join("ifc_geometry_items.csv"),
        render_geometry_items_csv(&report.geometry_items),
    )
    .with_context(|| format!("寫入 ifc_geometry_items.csv 失敗：{}", output_dir.display()))?;
    fs::write(output_dir.join("ifc_info.html"), render_info_html(&report))
        .with_context(|| format!("寫入 ifc_info.html 失敗：{}", output_dir.display()))?;
    Ok(report)
}

pub fn build_ifc_info_report(
    input_file: &Path,
    index: &StepIndex,
    converted_products: &[ConvertedProductInfo],
    coordinate_info: Option<IfcCoordinateInfo>,
) -> IfcInfoReport {
    let converted = converted_products
        .iter()
        .map(|item| (item.ifc_step_id, item))
        .collect::<HashMap<_, _>>();
    let entity_type_counts = entity_type_counts(index);
    let psets = collect_property_sets(index);
    let cores = collect_products(index);

    let mut products = Vec::with_capacity(cores.len());
    let mut properties = Vec::new();
    let mut geometry_items = Vec::new();

    for core in cores {
        let product_properties = psets.get(&core.ifc_step_id).cloned().unwrap_or_default();
        let product_geometry_items = collect_geometry_items(index, &core);
        let converted_product = converted.get(&core.ifc_step_id).copied();
        let triangle_count = converted_product
            .map(|product| product.triangle_count)
            .unwrap_or_default();
        for property in &product_properties {
            properties.push(IfcPropertyInfo {
                ifc_step_id: core.ifc_step_id,
                global_id: core.global_id.clone(),
                ifc_type: core.ifc_type.clone(),
                name: core.name.clone(),
                pset_name: property.pset_name.clone(),
                property_name: property.property_name.clone(),
                property_value: property.property_value.clone(),
            });
        }
        geometry_items.extend(product_geometry_items.iter().cloned());
        products.push(IfcProductInfo {
            ifc_step_id: core.ifc_step_id,
            global_id: core.global_id,
            ifc_type: core.ifc_type,
            name: core.name,
            description: core.description,
            object_type: core.object_type,
            tag: core.tag,
            representation_step_id: core.representation_step_id,
            converted: triangle_count > 0,
            triangle_count,
            wgs84_bounds_min: converted_product.and_then(|product| product.wgs84_bounds_min),
            wgs84_bounds_max: converted_product.and_then(|product| product.wgs84_bounds_max),
            wgs84_center: converted_product.and_then(|product| product.wgs84_center),
            property_count: product_properties.len(),
            geometry_item_count: product_geometry_items.len(),
        });
    }

    let converted_product_count = products.iter().filter(|product| product.converted).count();
    IfcInfoReport {
        input_file: input_file.display().to_string(),
        entity_count: index.len(),
        entity_type_counts,
        coordinate_info: coordinate_info.unwrap_or_default(),
        product_count: products.len(),
        converted_product_count,
        skipped_product_count: products.len().saturating_sub(converted_product_count),
        property_count: properties.len(),
        geometry_item_count: geometry_items.len(),
        products,
        properties,
        geometry_items,
    }
}

fn entity_type_counts(index: &StepIndex) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for entity in index.entities() {
        *counts.entry(entity.type_name.clone()).or_insert(0) += 1;
    }
    counts
}

fn collect_products(index: &StepIndex) -> Vec<ProductCore> {
    let mut products = Vec::new();
    for entity in index.entities() {
        let args = split_arguments(index.body(entity));
        let Some(representation_step_id) = args.get(6).and_then(|arg| extract_first_ref(arg))
        else {
            continue;
        };
        if index
            .entity(representation_step_id)
            .is_none_or(|entity| entity.type_name != "IFCPRODUCTDEFINITIONSHAPE")
        {
            continue;
        }
        products.push(ProductCore {
            ifc_step_id: entity.id,
            global_id: args
                .first()
                .map(|arg| decode_ifc_string(arg))
                .unwrap_or_default(),
            ifc_type: entity.type_name.clone(),
            name: args
                .get(2)
                .map(|arg| decode_ifc_string(arg))
                .unwrap_or_default(),
            description: args
                .get(3)
                .map(|arg| decode_ifc_string(arg))
                .unwrap_or_default(),
            object_type: args
                .get(4)
                .map(|arg| decode_ifc_string(arg))
                .unwrap_or_default(),
            tag: args
                .get(7)
                .map(|arg| decode_ifc_string(arg))
                .unwrap_or_default(),
            representation_step_id,
        });
    }
    products
}

fn collect_property_sets(index: &StepIndex) -> HashMap<u32, Vec<PropertyValue>> {
    let mut by_object = HashMap::<u32, Vec<PropertyValue>>::new();
    for rel in index.entities_by_type("IFCRELDEFINESBYPROPERTIES") {
        let args = split_arguments(index.body(rel));
        let object_ids = args.get(4).map(|arg| extract_refs(arg)).unwrap_or_default();
        let Some(pset_id) = args.get(5).and_then(|arg| extract_first_ref(arg)) else {
            continue;
        };
        let pset_values = collect_pset_values(index, pset_id);
        for object_id in object_ids {
            by_object
                .entry(object_id)
                .or_default()
                .extend(pset_values.iter().cloned());
        }
    }
    by_object
}

fn collect_pset_values(index: &StepIndex, pset_id: u32) -> Vec<PropertyValue> {
    let Some(pset) = index.entity(pset_id) else {
        return Vec::new();
    };
    if pset.type_name != "IFCPROPERTYSET" {
        return Vec::new();
    }
    let args = split_arguments(index.body(pset));
    let pset_name = args
        .get(2)
        .map(|arg| decode_ifc_string(arg))
        .unwrap_or_else(|| format!("#{pset_id}"));
    let property_ids = args.get(4).map(|arg| extract_refs(arg)).unwrap_or_default();
    let mut values = Vec::new();
    for property_id in property_ids {
        let Some(property) = index.entity(property_id) else {
            continue;
        };
        if property.type_name != "IFCPROPERTYSINGLEVALUE" {
            continue;
        }
        let property_args = split_arguments(index.body(property));
        values.push(PropertyValue {
            pset_name: pset_name.clone(),
            property_name: property_args
                .first()
                .map(|arg| decode_ifc_string(arg))
                .unwrap_or_else(|| format!("#{property_id}")),
            property_value: property_args
                .get(2)
                .map(|arg| property_value_text(arg))
                .unwrap_or_default(),
        });
    }
    values
}

fn collect_geometry_items(index: &StepIndex, product: &ProductCore) -> Vec<IfcGeometryItemInfo> {
    let mut rows = Vec::new();
    let Some(pds) = index.entity(product.representation_step_id) else {
        return rows;
    };
    let pds_args = split_arguments(index.body(pds));
    for shape_id in pds_args
        .get(2)
        .map(|arg| extract_refs(arg))
        .unwrap_or_default()
    {
        let Some(shape) = index.entity(shape_id) else {
            continue;
        };
        if shape.type_name != "IFCSHAPEREPRESENTATION" {
            continue;
        }
        let shape_args = split_arguments(index.body(shape));
        let representation_identifier = shape_args
            .get(1)
            .map(|arg| decode_ifc_string(arg))
            .unwrap_or_default();
        let representation_type = shape_args
            .get(2)
            .map(|arg| decode_ifc_string(arg))
            .unwrap_or_default();
        for item_id in shape_args
            .get(3)
            .map(|arg| extract_refs(arg))
            .unwrap_or_default()
        {
            let Some(item) = index.entity(item_id) else {
                continue;
            };
            let resolved_types = dedupe_preserve_order(resolve_geometry_item_types(index, item_id));
            let supported = resolved_types
                .iter()
                .any(|item_type| is_supported_geometry_type(item_type));
            rows.push(IfcGeometryItemInfo {
                ifc_step_id: product.ifc_step_id,
                global_id: product.global_id.clone(),
                ifc_type: product.ifc_type.clone(),
                name: product.name.clone(),
                representation_step_id: shape_id,
                representation_identifier: representation_identifier.clone(),
                representation_type: representation_type.clone(),
                item_step_id: item_id,
                item_type: item.type_name.clone(),
                resolved_item_type: if resolved_types.is_empty() {
                    item.type_name.clone()
                } else {
                    resolved_types.join("|")
                },
                supported,
            });
        }
    }
    rows
}

fn resolve_geometry_item_types(index: &StepIndex, item_id: u32) -> Vec<String> {
    let Some(item) = index.entity(item_id) else {
        return Vec::new();
    };
    if item.type_name != "IFCMAPPEDITEM" {
        if item.type_name == "IFCEXTRUDEDAREASOLID" {
            return vec![extruded_area_solid_type(index, item_id)];
        }
        return vec![item.type_name.clone()];
    }

    let item_args = split_arguments(index.body(item));
    let Some(map_id) = item_args.first().and_then(|arg| extract_first_ref(arg)) else {
        return vec![item.type_name.clone()];
    };
    let Some(map) = index.entity(map_id) else {
        return vec![item.type_name.clone()];
    };
    let map_args = split_arguments(index.body(map));
    let Some(mapped_rep_id) = map_args.get(1).and_then(|arg| extract_first_ref(arg)) else {
        return vec![item.type_name.clone()];
    };
    let Some(mapped_rep) = index.entity(mapped_rep_id) else {
        return vec![item.type_name.clone()];
    };
    let mapped_args = split_arguments(index.body(mapped_rep));
    let mut types = Vec::new();
    for child_id in mapped_args
        .get(3)
        .map(|arg| extract_refs(arg))
        .unwrap_or_default()
    {
        types.extend(resolve_geometry_item_types(index, child_id));
    }
    types
}

fn dedupe_preserve_order(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for value in values {
        if seen.insert(value.clone()) {
            deduped.push(value);
        }
    }
    deduped
}

fn extruded_area_solid_type(index: &StepIndex, item_id: u32) -> String {
    let Some(item) = index.entity(item_id) else {
        return "IFCEXTRUDEDAREASOLID".to_string();
    };
    let args = split_arguments(index.body(item));
    let Some(profile_id) = args.first().and_then(|arg| extract_first_ref(arg)) else {
        return "IFCEXTRUDEDAREASOLID".to_string();
    };
    let Some(profile) = index.entity(profile_id) else {
        return "IFCEXTRUDEDAREASOLID".to_string();
    };
    format!("IFCEXTRUDEDAREASOLID:{}", profile.type_name)
}

fn is_supported_geometry_type(item_type: &str) -> bool {
    matches!(
        item_type,
        "IFCFACETEDBREP" | "IFCFACEBASEDSURFACEMODEL" | "IFCSHELLBASEDSURFACEMODEL"
    ) || item_type == "IFCEXTRUDEDAREASOLID:IFCCIRCLEPROFILEDEF"
}

fn property_value_text(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed == "$" || trimmed == "*" {
        return String::new();
    }
    if let Some(open) = trimmed.find('(')
        && trimmed.ends_with(')')
    {
        return decode_ifc_string(&trimmed[open + 1..trimmed.len() - 1]);
    }
    decode_ifc_string(trimmed)
}

fn render_products_csv(products: &[IfcProductInfo]) -> String {
    let mut out = String::from(
        "ifc_step_id,global_id,ifc_type,name,description,object_type,tag,representation_step_id,converted,triangle_count,property_count,geometry_item_count,wgs84_min_lon,wgs84_min_lat,wgs84_min_height,wgs84_max_lon,wgs84_max_lat,wgs84_max_height,wgs84_center_lon,wgs84_center_lat,wgs84_center_height\n",
    );
    for product in products {
        let mut values = vec![
            product.ifc_step_id.to_string(),
            product.global_id.clone(),
            product.ifc_type.clone(),
            product.name.clone(),
            product.description.clone(),
            product.object_type.clone(),
            product.tag.clone(),
            product.representation_step_id.to_string(),
            product.converted.to_string(),
            product.triangle_count.to_string(),
            product.property_count.to_string(),
            product.geometry_item_count.to_string(),
        ];
        push_wgs84_csv_values(&mut values, product.wgs84_bounds_min);
        push_wgs84_csv_values(&mut values, product.wgs84_bounds_max);
        push_wgs84_csv_values(&mut values, product.wgs84_center);
        write_csv_row(&mut out, &values);
    }
    out
}

fn push_wgs84_csv_values(values: &mut Vec<String>, coord: Option<Wgs84Coordinate>) {
    if let Some(coord) = coord {
        values.push(coord.lon.to_string());
        values.push(coord.lat.to_string());
        values.push(coord.height.to_string());
    } else {
        values.extend(["".to_string(), "".to_string(), "".to_string()]);
    }
}

fn render_properties_csv(properties: &[IfcPropertyInfo]) -> String {
    let mut out = String::from(
        "ifc_step_id,global_id,ifc_type,name,pset_name,property_name,property_value\n",
    );
    for property in properties {
        write_csv_row(
            &mut out,
            &[
                property.ifc_step_id.to_string(),
                property.global_id.clone(),
                property.ifc_type.clone(),
                property.name.clone(),
                property.pset_name.clone(),
                property.property_name.clone(),
                property.property_value.clone(),
            ],
        );
    }
    out
}

fn render_geometry_items_csv(items: &[IfcGeometryItemInfo]) -> String {
    let mut out = String::from(
        "ifc_step_id,global_id,ifc_type,name,representation_step_id,representation_identifier,representation_type,item_step_id,item_type,resolved_item_type,supported\n",
    );
    for item in items {
        write_csv_row(
            &mut out,
            &[
                item.ifc_step_id.to_string(),
                item.global_id.clone(),
                item.ifc_type.clone(),
                item.name.clone(),
                item.representation_step_id.to_string(),
                item.representation_identifier.clone(),
                item.representation_type.clone(),
                item.item_step_id.to_string(),
                item.item_type.clone(),
                item.resolved_item_type.clone(),
                item.supported.to_string(),
            ],
        );
    }
    out
}

fn write_csv_row(out: &mut String, values: &[String]) {
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&escape_csv(value));
    }
    out.push('\n');
}

fn escape_csv(value: &str) -> String {
    let value = neutralize_csv_formula(value);
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value
    }
}

fn neutralize_csv_formula(value: &str) -> String {
    let trimmed = value.trim_start_matches([' ', '\t', '\r', '\n']);
    if matches!(trimmed.chars().next(), Some('=' | '+' | '-' | '@')) {
        format!("'{value}")
    } else {
        value.to_string()
    }
}

fn render_info_html(report: &IfcInfoReport) -> String {
    let coordinate_rows = render_coordinate_rows(&report.coordinate_info);
    let map_json = serde_json::to_string(&report.coordinate_info).unwrap_or_else(|_| "{}".into());
    let product_rows = report
        .products
        .iter()
        .map(|product| {
            format!(
                "<tr><td>#{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                product.ifc_step_id,
                escape_html(&product.ifc_type),
                escape_html(&product.name),
                product.converted,
                product.triangle_count,
                product.property_count,
                format_wgs84_html(product.wgs84_center)
            )
        })
        .collect::<String>();
    let property_rows = report
        .properties
        .iter()
        .map(|property| {
            format!(
                "<tr><td>#{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                property.ifc_step_id,
                escape_html(&property.name),
                escape_html(&property.pset_name),
                escape_html(&property.property_name),
                escape_html(&property.property_value)
            )
        })
        .collect::<String>();
    let geometry_rows = report
        .geometry_items
        .iter()
        .map(|item| {
            format!(
                "<tr><td>#{}</td><td>{}</td><td>{}</td><td>{}</td><td>#{}</td><td>{}</td><td>{}</td></tr>",
                item.ifc_step_id,
                escape_html(&item.name),
                escape_html(&item.representation_identifier),
                escape_html(&item.representation_type),
                item.item_step_id,
                escape_html(&item.resolved_item_type),
                item.supported
            )
        })
        .collect::<String>();
    let mut entity_counts = report
        .entity_type_counts
        .iter()
        .map(|(name, count)| (name.as_str(), *count))
        .collect::<Vec<_>>();
    entity_counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    let entity_rows = entity_counts
        .iter()
        .take(40)
        .map(|(name, count)| format!("<tr><td>{}</td><td>{}</td></tr>", escape_html(name), count))
        .collect::<String>();

    format!(
        r#"<!doctype html>
<html lang="zh-Hant">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>IFC Info Report</title>
  <style>
    body {{ margin: 24px; font-family: "Microsoft JhengHei", "Segoe UI", sans-serif; color: #182026; background: #f7fafc; }}
    h1 {{ margin: 0 0 8px; font-size: 24px; }}
    h2 {{ margin-top: 28px; font-size: 18px; }}
    .summary {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 10px; margin: 18px 0; }}
    .card {{ padding: 12px; border: 1px solid #d7e1e7; border-radius: 8px; background: white; }}
    .card strong {{ display: block; font-size: 22px; }}
    .coord-layout {{ display: grid; grid-template-columns: minmax(260px, 1fr) minmax(320px, 520px); gap: 12px; align-items: stretch; }}
    .mini-map {{ min-height: 280px; border: 1px solid #d7e1e7; background: #dfe8ec; position: relative; overflow: hidden; }}
    .mini-map-empty {{ padding: 16px; color: #60717c; }}
    .coord-line {{ display: block; white-space: nowrap; }}
    .coord-axis {{ display: inline-block; min-width: 28px; color: #60717c; }}
    table {{ width: 100%; border-collapse: collapse; margin: 8px 0 18px; background: white; }}
    th, td {{ padding: 7px 8px; border: 1px solid #d7e1e7; font-size: 12px; text-align: left; vertical-align: top; }}
    th {{ background: #eaf2f6; position: sticky; top: 0; }}
    .table-wrap {{ max-height: 55vh; overflow: auto; border: 1px solid #d7e1e7; }}
    .muted {{ color: #60717c; }}
    .search {{ width: min(520px, calc(100vw - 48px)); height: 34px; margin: 16px 0 8px; padding: 0 10px; border: 1px solid #b9c8d1; border-radius: 6px; font-size: 14px; }}
    code {{ font-family: Consolas, monospace; }}
  </style>
</head>
<body>
  <h1>IFC Info Report</h1>
  <div class="muted"><code>{}</code></div>
  <section class="summary">
    <div class="card">Entities<strong>{}</strong></div>
    <div class="card">Products<strong>{}</strong></div>
    <div class="card">Converted<strong>{}</strong></div>
    <div class="card">Skipped<strong>{}</strong></div>
    <div class="card">Properties<strong>{}</strong></div>
    <div class="card">Geometry Items<strong>{}</strong></div>
  </section>
  <h2>Coordinate Info</h2>
  <div class="coord-layout">
    <div class="table-wrap"><table><thead><tr><th>Set</th><th>Min</th><th>Max</th><th>Center / Origin</th></tr></thead><tbody>{}</tbody></table></div>
    <div id="miniMap" class="mini-map"><div class="mini-map-empty">沒有可定位的 WGS84 範圍；請先跑 IFC -> 3D Tiles 轉檔。</div></div>
  </div>
  <input id="searchInput" class="search" type="search" placeholder="搜尋 StepId、類型、名稱、Pset、geometry type">
  <h2>Products</h2>
  <div class="table-wrap"><table data-filterable><thead><tr><th>Step</th><th>Type</th><th>Name</th><th>Converted</th><th>Triangles</th><th>Properties</th><th>WGS84 Center</th></tr></thead><tbody>{}</tbody></table></div>
  <h2>Properties</h2>
  <div class="table-wrap"><table data-filterable><thead><tr><th>Step</th><th>Product</th><th>Pset</th><th>Property</th><th>Value</th></tr></thead><tbody>{}</tbody></table></div>
  <h2>Geometry Items</h2>
  <div class="table-wrap"><table data-filterable><thead><tr><th>Step</th><th>Product</th><th>Identifier</th><th>Representation</th><th>Item</th><th>Resolved Type</th><th>Supported</th></tr></thead><tbody>{}</tbody></table></div>
  <h2>Top Entity Types</h2>
  <div class="table-wrap"><table data-filterable><thead><tr><th>Entity Type</th><th>Count</th></tr></thead><tbody>{}</tbody></table></div>
  <script type="application/json" id="coordinateInfoData">{}</script>
  <script src="https://www.focusit.com.tw/easymap/easymap/easymap.js"></script>
  <script>
    const searchInput = document.getElementById("searchInput");
    searchInput.addEventListener("input", () => {{
      const q = searchInput.value.trim().toLowerCase();
      document.querySelectorAll("table[data-filterable] tbody tr").forEach(row => {{
        row.hidden = q && !row.textContent.toLowerCase().includes(q);
      }});
    }});
    function initMiniMap(attempt = 0) {{
      const container = document.getElementById("miniMap");
      const data = JSON.parse(document.getElementById("coordinateInfoData").textContent || "{{}}");
      if (!container || !data.wgs84_bounds_min || !data.wgs84_bounds_max || !data.wgs84_center) {{
        return;
      }}
      if (typeof Easymap === "undefined" || typeof dgWKT === "undefined") {{
        if (attempt < 80) {{
          window.setTimeout(() => initMiniMap(attempt + 1), 100);
        }}
        return;
      }}
      container.innerHTML = "";
      const west = Math.min(data.wgs84_bounds_min.lon, data.wgs84_bounds_max.lon);
      const east = Math.max(data.wgs84_bounds_min.lon, data.wgs84_bounds_max.lon);
      const south = Math.min(data.wgs84_bounds_min.lat, data.wgs84_bounds_max.lat);
      const north = Math.max(data.wgs84_bounds_min.lat, data.wgs84_bounds_max.lat);
      const center = data.wgs84_center;
      const span = Math.max(Math.abs(east - west), Math.abs(north - south));
      const zoom = span < 0.001 ? 17 : span < 0.005 ? 16 : span < 0.02 ? 15 : span < 0.08 ? 13 : span < 0.3 ? 11 : 9;
      const map = new Easymap("miniMap");
      const rectWkt = `POLYGON((${{west}} ${{south}},${{east}} ${{south}},${{east}} ${{north}},${{west}} ${{north}},${{west}} ${{south}}))`;
      const extent = new dgWKT([{{ label: "Model WGS84 extent", wkt: rectWkt }}], "EPSG:4326", function () {{}});
      const icon = new dgIcon("https://www.focusit.com.tw/easymap/easymap/7/imgs/marker.png", 28, 28);
      const marker = new dgMarker(new dgXY(center.lon, center.lat), icon, false);
      map.addItem([extent, marker]);
      map.zoomToXY(new dgXY(center.lon, center.lat), zoom);
    }}
    window.addEventListener("load", initMiniMap);
  </script>
</body>
</html>
"#,
        escape_html(&report.input_file),
        report.entity_count,
        report.product_count,
        report.converted_product_count,
        report.skipped_product_count,
        report.property_count,
        report.geometry_item_count,
        coordinate_rows,
        product_rows,
        property_rows,
        geometry_rows,
        entity_rows,
        map_json
    )
}

fn render_coordinate_rows(info: &IfcCoordinateInfo) -> String {
    format!(
        "<tr><td>WGS84</td><td>{}</td><td>{}</td><td><strong>center</strong>{}<br><strong>origin</strong>{}</td></tr>",
        format_wgs84_html(info.wgs84_bounds_min),
        format_wgs84_html(info.wgs84_bounds_max),
        format_wgs84_html(info.wgs84_center),
        format_wgs84_html(info.wgs84_origin)
    )
}

fn format_wgs84_html(coord: Option<Wgs84Coordinate>) -> String {
    coord.map_or_else(
        || "-".to_string(),
        |coord| {
            format!(
                "<span class=\"coord-line\"><span class=\"coord-axis\">lon</span>{}</span>\
                 <span class=\"coord-line\"><span class=\"coord-axis\">lat</span>{}</span>\
                 <span class=\"coord-line\"><span class=\"coord-axis\">h</span>{}</span>",
                escape_html(&coord.lon.to_string()),
                escape_html(&coord.lat.to_string()),
                escape_html(&coord.height.to_string())
            )
        },
    )
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn safe_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.trim().is_empty())
        .unwrap_or("ifc_info")
        .to_string()
}
