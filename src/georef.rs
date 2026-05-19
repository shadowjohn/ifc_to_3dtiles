use serde::{Deserialize, Serialize};

use crate::project::SourceStatus;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds2 {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

impl Bounds2 {
    pub fn new(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        Self {
            min_x,
            min_y,
            max_x,
            max_y,
        }
    }

    pub fn scaled(self, scale: f64) -> Self {
        Self {
            min_x: self.min_x * scale,
            min_y: self.min_y * scale,
            max_x: self.max_x * scale,
            max_y: self.max_y * scale,
        }
    }

    pub fn width(self) -> f64 {
        (self.max_x - self.min_x).abs()
    }

    pub fn height(self) -> f64 {
        (self.max_y - self.min_y).abs()
    }

    pub fn center(self) -> [f64; 2] {
        [
            (self.min_x + self.max_x) * 0.5,
            (self.min_y + self.max_y) * 0.5,
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundsSummary {
    pub raw_bounds: Bounds2,
    pub percentile_bounds: Bounds2,
    pub centroid: [f64; 2],
}

impl BoundsSummary {
    pub fn from_raw_and_percentile(raw_bounds: Bounds2, percentile_bounds: Bounds2) -> Self {
        Self {
            raw_bounds,
            percentile_bounds,
            centroid: percentile_bounds.center(),
        }
    }

    pub fn scaled(self, scale: f64) -> Self {
        Self {
            raw_bounds: self.raw_bounds.scaled(scale),
            percentile_bounds: self.percentile_bounds.scaled(scale),
            centroid: [self.centroid[0] * scale, self.centroid[1] * scale],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aoi {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

impl Aoi {
    pub fn new(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        Self {
            min_x,
            min_y,
            max_x,
            max_y,
        }
    }

    pub fn contains_bounds(self, bounds: Bounds2) -> bool {
        bounds.min_x >= self.min_x
            && bounds.max_x <= self.max_x
            && bounds.min_y >= self.min_y
            && bounds.max_y <= self.max_y
    }

    pub fn contains_point(self, point: [f64; 2]) -> bool {
        point[0] >= self.min_x
            && point[0] <= self.max_x
            && point[1] >= self.min_y
            && point[1] <= self.max_y
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScaleClassification {
    pub selected_scale: Option<f64>,
    pub status: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SourceStatusDecision {
    pub status: SourceStatus,
    pub selected_scale: Option<f64>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceTransform {
    pub source_crs: String,
    pub canonical_crs: String,
    pub translation: [f64; 3],
    pub rotation_quat_xyzw: [f64; 4],
    pub scale: [f64; 3],
    pub unit_scale_to_meter: f64,
    pub canonical_space: String,
}

impl SourceTransform {
    pub fn identity(source_crs: &str, unit_scale_to_meter: f64) -> Self {
        Self {
            source_crs: source_crs.to_string(),
            canonical_crs: "EPSG:3826".to_string(),
            translation: [0.0, 0.0, 0.0],
            rotation_quat_xyzw: [0.0, 0.0, 0.0, 1.0],
            scale: [
                unit_scale_to_meter,
                unit_scale_to_meter,
                unit_scale_to_meter,
            ],
            unit_scale_to_meter,
            canonical_space: "EPSG:3826 meters / local ENU / Z-up".to_string(),
        }
    }
}

pub fn classify_source_scale(
    bounds: &BoundsSummary,
    aoi: &Aoi,
    allowed_scales: &[f64],
) -> ScaleClassification {
    for scale in allowed_scales {
        let scaled = bounds.scaled(*scale);
        if aoi.contains_point(scaled.centroid)
            && aoi.contains_bounds(scaled.percentile_bounds)
            && scaled.percentile_bounds.width() > 0.01
            && scaled.percentile_bounds.height() > 0.01
        {
            let mut warnings = vec![];
            if !aoi.contains_bounds(scaled.raw_bounds) {
                warnings.push(
                    "raw bbox outside AOI; accepted by centroid and percentile bounds".to_string(),
                );
            }
            return ScaleClassification {
                selected_scale: Some(*scale),
                status: "inside_aoi".to_string(),
                warnings,
            };
        }
    }

    ScaleClassification {
        selected_scale: None,
        status: "outside_aoi".to_string(),
        warnings: vec!["source bounds outside AOI for all allowed scales".to_string()],
    }
}

pub fn decide_source_status(
    bounds_xy: BoundsSummary,
    z_range_m: f64,
    aoi: &Aoi,
    allowed_scales: &[f64],
) -> SourceStatusDecision {
    let scale = classify_source_scale(&bounds_xy, aoi, allowed_scales);
    let mut warnings = scale.warnings;

    if z_range_m.abs() < 0.05 {
        warnings.push("source appears 2D because z range is below 5cm".to_string());
        return SourceStatusDecision {
            status: SourceStatus::Quarantined,
            selected_scale: scale.selected_scale,
            warnings,
        };
    }

    if scale.selected_scale.is_none() {
        return SourceStatusDecision {
            status: SourceStatus::Quarantined,
            selected_scale: None,
            warnings,
        };
    }

    SourceStatusDecision {
        status: SourceStatus::Approved,
        selected_scale: scale.selected_scale,
        warnings,
    }
}
