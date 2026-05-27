use std::collections::HashMap;

use anyhow::{Result, anyhow, bail};

use crate::step::{StepIndex, extract_first_ref, extract_refs, numbers_in, split_arguments};

const CIRCLE_EXTRUSION_SEGMENT_CHOICES: [usize; 3] = [24, 48, 64];
const DEFAULT_CIRCLE_EXTRUSION_MAX_SAGITTA: f64 = 0.025;
const SURFACE_AWARE_MAX_SMOOTH_ANGLE_DEG: f64 = 60.0;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    pub fn cross(self, other: Self) -> Self {
        Self {
            x: self.y * other.z - self.z * other.y,
            y: self.z * other.x - self.x * other.z,
            z: self.x * other.y - self.y * other.x,
        }
    }

    pub fn length(self) -> f64 {
        self.dot(self).sqrt()
    }

    pub fn normalized(self) -> Self {
        let len = self.length();
        if len <= f64::EPSILON {
            self
        } else {
            self / len
        }
    }

    pub fn to_array(self) -> [f64; 3] {
        [self.x, self.y, self.z]
    }
}

impl std::ops::Add for Vec3 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
    }
}

impl std::ops::Sub for Vec3 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(self.x - rhs.x, self.y - rhs.y, self.z - rhs.z)
    }
}

impl std::ops::Mul<f64> for Vec3 {
    type Output = Self;
    fn mul(self, rhs: f64) -> Self::Output {
        Self::new(self.x * rhs, self.y * rhs, self.z * rhs)
    }
}

impl std::ops::Div<f64> for Vec3 {
    type Output = Self;
    fn div(self, rhs: f64) -> Self::Output {
        Self::new(self.x / rhs, self.y / rhs, self.z / rhs)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mat4 {
    pub m: [[f64; 4]; 4],
}

impl Mat4 {
    pub fn identity() -> Self {
        Self {
            m: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    pub fn from_basis(origin: Vec3, x: Vec3, y: Vec3, z: Vec3) -> Self {
        Self {
            m: [
                [x.x, y.x, z.x, origin.x],
                [x.y, y.y, z.y, origin.y],
                [x.z, y.z, z.z, origin.z],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    pub fn transform_point(self, p: Vec3) -> Vec3 {
        Vec3 {
            x: self.m[0][0] * p.x + self.m[0][1] * p.y + self.m[0][2] * p.z + self.m[0][3],
            y: self.m[1][0] * p.x + self.m[1][1] * p.y + self.m[1][2] * p.z + self.m[1][3],
            z: self.m[2][0] * p.x + self.m[2][1] * p.y + self.m[2][2] * p.z + self.m[2][3],
        }
    }
}

impl std::ops::Mul for Mat4 {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        let mut out = [[0.0; 4]; 4];
        for (r, row) in out.iter_mut().enumerate() {
            for (c, value) in row.iter_mut().enumerate() {
                *value = self.m[r][0] * rhs.m[0][c]
                    + self.m[r][1] * rhs.m[1][c]
                    + self.m[r][2] * rhs.m[2][c]
                    + self.m[r][3] * rhs.m[3][c];
            }
        }
        Self { m: out }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Bounds {
    pub min: Vec3,
    pub max: Vec3,
}

impl Bounds {
    pub fn empty() -> Self {
        Self {
            min: Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
            max: Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
        }
    }

    pub fn include(&mut self, p: Vec3) {
        self.min.x = self.min.x.min(p.x);
        self.min.y = self.min.y.min(p.y);
        self.min.z = self.min.z.min(p.z);
        self.max.x = self.max.x.max(p.x);
        self.max.y = self.max.y.max(p.y);
        self.max.z = self.max.z.max(p.z);
    }

    pub fn include_bounds(&mut self, other: &Bounds) {
        if other.is_valid() {
            self.include(other.min);
            self.include(other.max);
        }
    }

    pub fn is_valid(&self) -> bool {
        self.min.x.is_finite() && self.max.x.is_finite()
    }

    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    pub fn size(&self) -> Vec3 {
        self.max - self.min
    }
}

#[derive(Debug, Clone)]
pub struct Mesh {
    pub positions: Vec<[f64; 3]>,
    pub normals: Vec<[f64; 3]>,
    pub colors: Vec<[f32; 4]>,
    pub batch_ids: Vec<u16>,
    pub bounds: Bounds,
}

impl Mesh {
    pub fn new() -> Self {
        Self {
            positions: Vec::new(),
            normals: Vec::new(),
            colors: Vec::new(),
            batch_ids: Vec::new(),
            bounds: Bounds::empty(),
        }
    }

    pub fn triangle_count(&self) -> usize {
        self.positions.len() / 3
    }

    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }

    pub fn append_with_batch(&mut self, other: &Mesh, batch_id: u16) {
        self.positions.extend_from_slice(&other.positions);
        self.normals.extend_from_slice(&other.normals);
        self.colors.extend_from_slice(&other.colors);
        self.batch_ids
            .extend(std::iter::repeat_n(batch_id, other.positions.len()));
        self.bounds.include_bounds(&other.bounds);
    }

    pub fn translate(&mut self, delta: Vec3) {
        self.bounds = Bounds::empty();
        for p in &mut self.positions {
            p[0] += delta.x;
            p[1] += delta.y;
            p[2] += delta.z;
            self.bounds.include(Vec3::new(p[0], p[1], p[2]));
        }
    }

    pub fn with_smoothed_normals_by_position(&self, tolerance: f64) -> Self {
        self.with_smoothed_normals_by_position_angle(tolerance, 180.0)
    }

    pub fn with_smoothed_normals_by_position_angle(&self, tolerance: f64, angle_deg: f64) -> Self {
        let tolerance = if tolerance.is_finite() && tolerance > 0.0 {
            tolerance
        } else {
            1e-6
        };
        let cos_limit = angle_deg.clamp(0.0, 180.0).to_radians().cos();
        let mut groups = HashMap::<(u16, i64, i64, i64), Vec<(usize, Vec3)>>::new();

        for (index, position) in self.positions.iter().enumerate() {
            let normal = self
                .normals
                .get(index)
                .map(|normal| Vec3::new(normal[0], normal[1], normal[2]).normalized())
                .unwrap_or_else(|| Vec3::new(0.0, 0.0, 1.0));
            let batch_id = self.batch_ids.get(index).copied().unwrap_or_default();
            groups
                .entry(normal_key(batch_id, position, tolerance))
                .or_default()
                .push((index, normal));
        }

        let mut smoothed = self.clone();
        for values in groups.values() {
            for (index, source_normal) in values {
                let mut sum = Vec3::default();
                for (_, candidate) in values {
                    if source_normal.dot(*candidate) >= cos_limit {
                        sum = sum + *candidate;
                    }
                }
                let normal = if sum.length() > f64::EPSILON {
                    sum.normalized()
                } else {
                    *source_normal
                };
                if let Some(target) = smoothed.normals.get_mut(*index) {
                    *target = normal.to_array();
                }
            }
        }

        smoothed
    }

    pub fn with_surface_aware_smoothed_normals(
        &self,
        tolerance: f64,
        smooth_angle_deg: f64,
    ) -> Self {
        let tolerance = if tolerance.is_finite() && tolerance > 0.0 {
            tolerance
        } else {
            1e-6
        };
        let smooth_angle_deg = smooth_angle_deg
            .clamp(0.0, 180.0)
            .min(SURFACE_AWARE_MAX_SMOOTH_ANGLE_DEG);
        let cos_limit = smooth_angle_deg.to_radians().cos();
        let mut groups = HashMap::<(u16, i64, i64, i64), Vec<NormalSample>>::new();

        for sample in self.weighted_normal_samples() {
            let Some(position) = self.positions.get(sample.index) else {
                continue;
            };
            let batch_id = self
                .batch_ids
                .get(sample.index)
                .copied()
                .unwrap_or_default();
            groups
                .entry(normal_key(batch_id, position, tolerance))
                .or_default()
                .push(sample);
        }

        let mut smoothed = self.clone();
        for values in groups.values() {
            for source in values {
                let mut sum = Vec3::default();
                for candidate in values {
                    let alignment = source.normal.dot(candidate.normal);
                    if alignment >= cos_limit {
                        sum = sum + candidate.normal * candidate.weight;
                    }
                }
                let normal = if sum.length() > f64::EPSILON {
                    sum.normalized()
                } else {
                    source.normal
                };
                if let Some(target) = smoothed.normals.get_mut(source.index) {
                    *target = normal.to_array();
                }
            }
        }

        smoothed
    }

    fn weighted_normal_samples(&self) -> Vec<NormalSample> {
        let mut samples = Vec::with_capacity(self.positions.len());
        for index in 0..self.positions.len() {
            samples.push(NormalSample {
                index,
                normal: self.vertex_normal_or_default(index),
                weight: 1.0,
            });
        }

        for triangle_start in (0..self.positions.len()).step_by(3) {
            if triangle_start + 2 >= self.positions.len() {
                break;
            }
            let a = Vec3::new(
                self.positions[triangle_start][0],
                self.positions[triangle_start][1],
                self.positions[triangle_start][2],
            );
            let b = Vec3::new(
                self.positions[triangle_start + 1][0],
                self.positions[triangle_start + 1][1],
                self.positions[triangle_start + 1][2],
            );
            let c = Vec3::new(
                self.positions[triangle_start + 2][0],
                self.positions[triangle_start + 2][1],
                self.positions[triangle_start + 2][2],
            );
            let area = (b - a).cross(c - a).length() * 0.5;
            if area <= f64::EPSILON {
                continue;
            }

            let angles = [
                corner_angle(a, b, c),
                corner_angle(b, c, a),
                corner_angle(c, a, b),
            ];
            for (offset, angle) in angles.into_iter().enumerate() {
                if let Some(sample) = samples.get_mut(triangle_start + offset) {
                    sample.weight = (area * angle.max(1e-9)).max(1e-9);
                }
            }
        }

        samples
    }

    fn vertex_normal_or_default(&self, index: usize) -> Vec3 {
        self.normals
            .get(index)
            .map(|normal| Vec3::new(normal[0], normal[1], normal[2]).normalized())
            .filter(|normal| normal.length() > f64::EPSILON)
            .unwrap_or_else(|| Vec3::new(0.0, 0.0, 1.0))
    }
}

#[derive(Debug, Clone, Copy)]
struct NormalSample {
    index: usize,
    normal: Vec3,
    weight: f64,
}

fn normal_key(batch_id: u16, position: &[f64; 3], tolerance: f64) -> (u16, i64, i64, i64) {
    (
        batch_id,
        (position[0] / tolerance).round() as i64,
        (position[1] / tolerance).round() as i64,
        (position[2] / tolerance).round() as i64,
    )
}

fn corner_angle(origin: Vec3, a: Vec3, b: Vec3) -> f64 {
    let va = a - origin;
    let vb = b - origin;
    let denom = va.length() * vb.length();
    if denom <= f64::EPSILON {
        return 0.0;
    }
    (va.dot(vb) / denom).clamp(-1.0, 1.0).acos()
}

pub fn circle_extrusion_segments_for_radius(radius: f64, max_sagitta: f64) -> usize {
    let radius = radius.abs();
    if radius <= f64::EPSILON {
        return CIRCLE_EXTRUSION_SEGMENT_CHOICES[0];
    }
    let max_sagitta = if max_sagitta.is_finite() && max_sagitta > 0.0 {
        max_sagitta
    } else {
        DEFAULT_CIRCLE_EXTRUSION_MAX_SAGITTA
    };
    for segments in CIRCLE_EXTRUSION_SEGMENT_CHOICES {
        let sagitta = radius * (1.0 - (std::f64::consts::PI / segments as f64).cos());
        if sagitta <= max_sagitta {
            return segments;
        }
    }
    *CIRCLE_EXTRUSION_SEGMENT_CHOICES.last().unwrap()
}

impl Default for Mesh {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MeshBuildOptions {
    pub batch_id: u16,
    pub color: [f32; 4],
}

pub fn parse_cartesian_point(index: &StepIndex, id: u32) -> Result<Vec3> {
    let entity = index
        .entity(id)
        .ok_or_else(|| anyhow!("missing point #{id}"))?;
    let nums = numbers_in(index.body(entity));
    if nums.len() < 2 {
        bail!("point #{id} has fewer than two coordinates");
    }
    Ok(Vec3::new(
        nums[0],
        nums[1],
        nums.get(2).copied().unwrap_or(0.0),
    ))
}

pub fn parse_direction(index: &StepIndex, id: u32) -> Result<Vec3> {
    let entity = index
        .entity(id)
        .ok_or_else(|| anyhow!("missing direction #{id}"))?;
    let nums = numbers_in(index.body(entity));
    if nums.len() < 2 {
        bail!("direction #{id} has fewer than two components");
    }
    Ok(Vec3::new(nums[0], nums[1], nums.get(2).copied().unwrap_or(0.0)).normalized())
}

pub fn axis2_placement_3d(index: &StepIndex, id: u32) -> Result<Mat4> {
    let entity = index
        .entity(id)
        .ok_or_else(|| anyhow!("missing axis placement #{id}"))?;
    let args = split_arguments(index.body(entity));
    let origin = args
        .first()
        .and_then(|arg| extract_first_ref(arg))
        .map(|ref_id| parse_cartesian_point(index, ref_id))
        .transpose()?
        .unwrap_or_default();
    let z = args
        .get(1)
        .and_then(|arg| extract_first_ref(arg))
        .map(|ref_id| parse_direction(index, ref_id))
        .transpose()?
        .unwrap_or_else(|| Vec3::new(0.0, 0.0, 1.0));
    let mut x = args
        .get(2)
        .and_then(|arg| extract_first_ref(arg))
        .map(|ref_id| parse_direction(index, ref_id))
        .transpose()?
        .unwrap_or_else(|| Vec3::new(1.0, 0.0, 0.0));
    let mut y = z.cross(x).normalized();
    if y.length() <= f64::EPSILON {
        x = Vec3::new(1.0, 0.0, 0.0);
        y = z.cross(x).normalized();
    }
    x = y.cross(z).normalized();
    Ok(Mat4::from_basis(origin, x, y, z.normalized()))
}

fn axis2_placement_2d(index: &StepIndex, id: u32) -> Result<Mat4> {
    let entity = index
        .entity(id)
        .ok_or_else(|| anyhow!("missing 2d axis placement #{id}"))?;
    let args = split_arguments(index.body(entity));
    let origin = args
        .first()
        .and_then(|arg| extract_first_ref(arg))
        .map(|ref_id| parse_cartesian_point(index, ref_id))
        .transpose()?
        .unwrap_or_default();
    let mut x = args
        .get(1)
        .and_then(|arg| extract_first_ref(arg))
        .map(|ref_id| parse_direction(index, ref_id))
        .transpose()?
        .unwrap_or_else(|| Vec3::new(1.0, 0.0, 0.0));
    x = Vec3::new(x.x, x.y, 0.0).normalized();
    if x.length() <= f64::EPSILON {
        x = Vec3::new(1.0, 0.0, 0.0);
    }
    let y = Vec3::new(-x.y, x.x, 0.0).normalized();
    Ok(Mat4::from_basis(
        Vec3::new(origin.x, origin.y, 0.0),
        x,
        y,
        Vec3::new(0.0, 0.0, 1.0),
    ))
}

pub fn local_placement_matrix(index: &StepIndex, id: u32) -> Result<Mat4> {
    fn inner(
        index: &StepIndex,
        id: u32,
        memo: &mut HashMap<u32, Mat4>,
        visiting: &mut Vec<u32>,
    ) -> Result<Mat4> {
        if let Some(value) = memo.get(&id) {
            return Ok(*value);
        }
        if visiting.contains(&id) {
            bail!("cyclic local placement at #{id}");
        }
        visiting.push(id);
        let entity = index
            .entity(id)
            .ok_or_else(|| anyhow!("missing local placement #{id}"))?;
        let args = split_arguments(index.body(entity));
        let parent = args
            .first()
            .and_then(|arg| extract_first_ref(arg))
            .map(|parent_id| inner(index, parent_id, memo, visiting))
            .transpose()?
            .unwrap_or_else(Mat4::identity);
        let relative = args
            .get(1)
            .and_then(|arg| extract_first_ref(arg))
            .map(|axis_id| axis2_placement_3d(index, axis_id))
            .transpose()?
            .unwrap_or_else(Mat4::identity);
        visiting.pop();
        let matrix = parent * relative;
        memo.insert(id, matrix);
        Ok(matrix)
    }

    inner(index, id, &mut HashMap::new(), &mut Vec::new())
}

pub fn cartesian_operator_3d(index: &StepIndex, id: u32) -> Result<Mat4> {
    let entity = index
        .entity(id)
        .ok_or_else(|| anyhow!("missing transformation operator #{id}"))?;
    let args = split_arguments(index.body(entity));
    let x = args
        .first()
        .and_then(|arg| extract_first_ref(arg))
        .map(|ref_id| parse_direction(index, ref_id))
        .transpose()?
        .unwrap_or_else(|| Vec3::new(1.0, 0.0, 0.0));
    let y = args
        .get(1)
        .and_then(|arg| extract_first_ref(arg))
        .map(|ref_id| parse_direction(index, ref_id))
        .transpose()?
        .unwrap_or_else(|| Vec3::new(0.0, 1.0, 0.0));
    let origin = args
        .get(2)
        .and_then(|arg| extract_first_ref(arg))
        .map(|ref_id| parse_cartesian_point(index, ref_id))
        .transpose()?
        .unwrap_or_default();
    let scale = args
        .get(3)
        .and_then(|arg| numbers_in(arg).first().copied())
        .unwrap_or(1.0);
    let z = args
        .get(4)
        .and_then(|arg| extract_first_ref(arg))
        .map(|ref_id| parse_direction(index, ref_id))
        .transpose()?
        .unwrap_or_else(|| x.cross(y).normalized());
    Ok(Mat4::from_basis(
        origin,
        x.normalized() * scale,
        y.normalized() * scale,
        z.normalized() * scale,
    ))
}

pub fn item_uses_projected_coordinates(index: &StepIndex, item_id: u32) -> bool {
    let mut stack = vec![item_id];
    let mut seen = HashMap::<u32, ()>::new();
    let mut visits = 0usize;

    while let Some(id) = stack.pop() {
        if seen.insert(id, ()).is_some() {
            continue;
        }
        visits += 1;
        if visits > 20_000 {
            return false;
        }
        let Some(entity) = index.entity(id) else {
            continue;
        };
        if entity.type_name == "IFCCARTESIANPOINT" {
            if let Ok(point) = parse_cartesian_point(index, id) {
                return point.x.abs() > 10_000.0 && point.y.abs() > 10_000.0;
            }
            return false;
        }
        let mut refs = extract_refs(index.body(entity));
        refs.reverse();
        stack.extend(refs);
    }

    false
}

pub fn mesh_faceted_brep(
    index: &StepIndex,
    brep_id: u32,
    transform: &Mat4,
    options: MeshBuildOptions,
) -> Result<Mesh> {
    let brep = index
        .entity(brep_id)
        .ok_or_else(|| anyhow!("missing faceted brep #{brep_id}"))?;
    let shell_id = extract_first_ref(index.body(brep))
        .ok_or_else(|| anyhow!("faceted brep #{brep_id} has no shell"))?;
    mesh_shell(index, shell_id, transform, options)
}

pub fn mesh_shell_based_surface_model(
    index: &StepIndex,
    surface_model_id: u32,
    transform: &Mat4,
    options: MeshBuildOptions,
) -> Result<Mesh> {
    let surface_model = index
        .entity(surface_model_id)
        .ok_or_else(|| anyhow!("missing shell based surface model #{surface_model_id}"))?;
    let shell_ids = extract_refs(index.body(surface_model));
    let mut mesh = Mesh::new();
    for shell_id in shell_ids {
        let shell_mesh = mesh_shell(index, shell_id, transform, options)?;
        mesh.append_with_batch(&shell_mesh, options.batch_id);
    }
    Ok(mesh)
}

pub fn mesh_face_based_surface_model(
    index: &StepIndex,
    surface_model_id: u32,
    transform: &Mat4,
    options: MeshBuildOptions,
) -> Result<Mesh> {
    let surface_model = index
        .entity(surface_model_id)
        .ok_or_else(|| anyhow!("missing face based surface model #{surface_model_id}"))?;
    let face_set_ids = extract_refs(index.body(surface_model));
    let mut mesh = Mesh::new();
    for face_set_id in face_set_ids {
        let face_set_mesh = mesh_shell(index, face_set_id, transform, options)?;
        mesh.append_with_batch(&face_set_mesh, options.batch_id);
    }
    Ok(mesh)
}

pub fn mesh_extruded_area_solid(
    index: &StepIndex,
    solid_id: u32,
    transform: &Mat4,
    options: MeshBuildOptions,
) -> Result<Mesh> {
    let solid = index
        .entity(solid_id)
        .ok_or_else(|| anyhow!("missing extruded area solid #{solid_id}"))?;
    let args = split_arguments(index.body(solid));
    let profile_id = args
        .first()
        .and_then(|arg| extract_first_ref(arg))
        .ok_or_else(|| anyhow!("extruded area solid #{solid_id} has no swept area"))?;
    let position = args
        .get(1)
        .and_then(|arg| extract_first_ref(arg))
        .map(|id| axis2_placement_3d(index, id))
        .transpose()?
        .unwrap_or_else(Mat4::identity);
    let direction = args
        .get(2)
        .and_then(|arg| extract_first_ref(arg))
        .map(|id| parse_direction(index, id))
        .transpose()?
        .unwrap_or_else(|| Vec3::new(0.0, 0.0, 1.0));
    let depth = args
        .get(3)
        .and_then(|arg| numbers_in(arg).first().copied())
        .unwrap_or(0.0);
    if depth <= f64::EPSILON {
        return Ok(Mesh::new());
    }

    let profile = index
        .entity(profile_id)
        .ok_or_else(|| anyhow!("missing swept area profile #{profile_id}"))?;
    if profile.type_name != "IFCCIRCLEPROFILEDEF" {
        return Ok(Mesh::new());
    }

    mesh_circle_extrusion(
        index,
        profile_id,
        &(*transform * position),
        direction.normalized() * depth,
        options,
    )
}

pub fn mesh_shell(
    index: &StepIndex,
    shell_id: u32,
    transform: &Mat4,
    options: MeshBuildOptions,
) -> Result<Mesh> {
    let shell = index
        .entity(shell_id)
        .ok_or_else(|| anyhow!("missing shell #{shell_id}"))?;
    let face_ids = extract_refs(index.body(shell));
    let mut mesh = Mesh::new();

    for face_id in face_ids {
        let Some(face) = index.entity(face_id) else {
            continue;
        };
        let bound_ids = extract_refs(index.body(face));
        let mut chosen_loop = None;
        for bound_id in &bound_ids {
            let Some(bound) = index.entity(*bound_id) else {
                continue;
            };
            if bound.type_name == "IFCFACEOUTERBOUND" {
                chosen_loop = extract_first_ref(index.body(bound));
                break;
            }
            if chosen_loop.is_none() {
                chosen_loop = extract_first_ref(index.body(bound));
            }
        }
        let Some(loop_id) = chosen_loop else {
            continue;
        };
        let Some(poly_loop) = index.entity(loop_id) else {
            continue;
        };
        let point_ids = extract_refs(index.body(poly_loop));
        if point_ids.len() < 3 {
            continue;
        }
        let mut points = Vec::with_capacity(point_ids.len());
        for point_id in point_ids {
            points.push(transform.transform_point(parse_cartesian_point(index, point_id)?));
        }
        append_fan(&mut mesh, &points, options.batch_id, options.color);
    }

    Ok(mesh)
}

fn append_fan(mesh: &mut Mesh, points: &[Vec3], batch_id: u16, color: [f32; 4]) {
    let first = points[0];
    for i in 1..points.len() - 1 {
        append_triangle(mesh, first, points[i], points[i + 1], batch_id, color);
    }
}

fn mesh_circle_extrusion(
    index: &StepIndex,
    profile_id: u32,
    transform: &Mat4,
    extrusion: Vec3,
    options: MeshBuildOptions,
) -> Result<Mesh> {
    let profile = index
        .entity(profile_id)
        .ok_or_else(|| anyhow!("missing circle profile #{profile_id}"))?;
    let args = split_arguments(index.body(profile));
    let radius = args
        .get(3)
        .and_then(|arg| numbers_in(arg).first().copied())
        .unwrap_or(0.0);
    if radius <= f64::EPSILON {
        return Ok(Mesh::new());
    }
    let profile_position = args
        .get(2)
        .and_then(|arg| extract_first_ref(arg))
        .map(|id| axis2_placement_2d(index, id))
        .transpose()?
        .unwrap_or_else(Mat4::identity);

    let segments =
        circle_extrusion_segments_for_radius(radius, DEFAULT_CIRCLE_EXTRUSION_MAX_SAGITTA);
    let mut local_points = Vec::with_capacity(segments);
    for index in 0..segments {
        let angle = std::f64::consts::TAU * index as f64 / segments as f64;
        local_points.push(profile_position.transform_point(Vec3::new(
            radius * angle.cos(),
            radius * angle.sin(),
            0.0,
        )));
    }
    let local_center = profile_position.transform_point(Vec3::new(0.0, 0.0, 0.0));
    let base_center = transform.transform_point(local_center);
    let top_center = transform.transform_point(local_center + extrusion);
    let base: Vec<Vec3> = local_points
        .iter()
        .map(|point| transform.transform_point(*point))
        .collect();
    let top: Vec<Vec3> = local_points
        .iter()
        .map(|point| transform.transform_point(*point + extrusion))
        .collect();

    let mut mesh = Mesh::new();
    for i in 0..segments {
        let j = (i + 1) % segments;
        append_triangle(
            &mut mesh,
            base[i],
            base[j],
            top[j],
            options.batch_id,
            options.color,
        );
        append_triangle(
            &mut mesh,
            base[i],
            top[j],
            top[i],
            options.batch_id,
            options.color,
        );
        append_triangle(
            &mut mesh,
            base_center,
            base[j],
            base[i],
            options.batch_id,
            options.color,
        );
        append_triangle(
            &mut mesh,
            top_center,
            top[i],
            top[j],
            options.batch_id,
            options.color,
        );
    }
    Ok(mesh)
}

fn append_triangle(mesh: &mut Mesh, a: Vec3, b: Vec3, c: Vec3, batch_id: u16, color: [f32; 4]) {
    let normal = (b - a).cross(c - a).normalized();
    if normal.length() <= f64::EPSILON {
        return;
    }
    for p in [a, b, c] {
        mesh.positions.push(p.to_array());
        mesh.normals.push(normal.to_array());
        mesh.colors.push(color);
        mesh.batch_ids.push(batch_id);
        mesh.bounds.include(p);
    }
}
