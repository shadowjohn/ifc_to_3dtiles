use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RvtExportJob {
    pub input_rvt: PathBuf,
    pub output_ifc: PathBuf,
    pub result_json: PathBuf,
    pub options: RvtExportOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RvtExportOptions {
    pub file_version: String,
    pub export_ifc_common_property_sets: bool,
    pub export_internal_revit_property_sets: bool,
    pub export_base_quantities: bool,
    pub export_material_psets: bool,
    pub export_user_defined_psets: bool,
    pub export_schedules_as_psets: bool,
    pub use_active_view_geometry: bool,
    pub visible_elements_of_current_view: bool,
    pub tessellation_level_of_detail: f64,
}

impl Default for RvtExportOptions {
    fn default() -> Self {
        Self {
            file_version: "IFC2x3CV2".to_string(),
            export_ifc_common_property_sets: true,
            export_internal_revit_property_sets: true,
            export_base_quantities: true,
            export_material_psets: true,
            export_user_defined_psets: false,
            export_schedules_as_psets: false,
            use_active_view_geometry: false,
            visible_elements_of_current_view: false,
            tessellation_level_of_detail: 0.5,
        }
    }
}
