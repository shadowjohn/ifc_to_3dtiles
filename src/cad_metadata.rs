use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CadHierarchyDump {
    pub source_id: String,
    pub models: Vec<CadModel>,
    pub references: Vec<CadReference>,
    pub levels: Vec<CadLevel>,
    pub cells: Vec<CadCell>,
    pub shared_cells: Vec<CadCell>,
    pub attachments: Vec<CadAttachment>,
    pub element_classes: Vec<String>,
    pub materials: Vec<CadMaterial>,
    pub line_styles: Vec<String>,
    pub warnings: Vec<String>,
}

impl CadHierarchyDump {
    pub fn empty_for_source(source_id: impl Into<String>) -> Self {
        Self {
            source_id: source_id.into(),
            models: vec![],
            references: vec![],
            levels: vec![],
            cells: vec![],
            shared_cells: vec![],
            attachments: vec![],
            element_classes: vec![],
            materials: vec![],
            line_styles: vec![],
            warnings: vec![
                "CAD hierarchy probe unavailable; reference/model/level/cell/material metadata is incomplete."
                    .to_string(),
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CadModel {
    pub name: String,
    pub element_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CadReference {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CadLevel {
    pub name: String,
    pub element_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CadCell {
    pub name: String,
    pub element_count: usize,
    pub center: Option<[f64; 3]>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CadAttachment {
    pub name: String,
    pub path: String,
    pub transform_hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CadMaterial {
    pub name: String,
    pub color_rgba: [f32; 4],
}
