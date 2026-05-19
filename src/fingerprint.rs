use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeometryFingerprint {
    pub source_id: String,
    pub vertex_count: u64,
    pub triangle_count: u64,
    pub bbox: [f64; 6],
    pub surface_area_m2: f64,
    pub hash: String,
}

pub fn duplicate_candidate_score(a: &GeometryFingerprint, b: &GeometryFingerprint) -> f64 {
    let tri_ratio = ratio_score(a.triangle_count as f64, b.triangle_count as f64);
    let vertex_ratio = ratio_score(a.vertex_count as f64, b.vertex_count as f64);
    let area_ratio = ratio_score(a.surface_area_m2, b.surface_area_m2);
    let bbox_ratio = bbox_similarity(a.bbox, b.bbox);
    (tri_ratio + vertex_ratio + area_ratio + bbox_ratio) / 4.0
}

fn ratio_score(a: f64, b: f64) -> f64 {
    if a <= 0.0 || b <= 0.0 {
        return 0.0;
    }
    a.min(b) / a.max(b)
}

fn bbox_similarity(a: [f64; 6], b: [f64; 6]) -> f64 {
    let mut total = 0.0;
    for i in 0..6 {
        let scale = a[i].abs().max(b[i].abs()).max(1.0);
        total += 1.0 - ((a[i] - b[i]).abs() / scale).min(1.0);
    }
    total / 6.0
}
