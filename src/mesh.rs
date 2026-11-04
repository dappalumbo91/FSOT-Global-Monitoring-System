//! Sphere and graticule. Positions go through [`crate::geo`], so the mesh, the
//! texture, and the labels share one lat/lon convention.

use crate::geo::{self, EARTH_R};
use std::f32::consts::PI;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LineVertex {
    pub position: [f32; 3],
    pub color: [f32; 4],
}

pub struct Mesh<V> {
    pub vertices: Vec<V>,
    pub indices: Vec<u32>,
}

pub fn uv_sphere(stacks: u32, slices: u32, radius: f32) -> Mesh<Vertex> {
    let mut vertices = Vec::with_capacity(((stacks + 1) * (slices + 1)) as usize);
    for stack in 0..=stacks {
        let v = stack as f32 / stacks as f32;
        let lat = PI * 0.5 - v * PI;
        for slice in 0..=slices {
            let u = slice as f32 / slices as f32;
            let lon = u * std::f32::consts::TAU - PI;
            let dir = geo::lat_lon_to_dir(lat, lon);
            vertices.push(Vertex {
                position: (dir * radius).to_array(),
                normal: dir.to_array(),
                uv: geo::equirect_uv(lat, lon),
            });
        }
    }

    let mut indices = Vec::with_capacity((stacks * slices * 6) as usize);
    let row = slices + 1;
    for stack in 0..stacks {
        for slice in 0..slices {
            let a = stack * row + slice;
            let b = a + row;
            // Counter-clockwise when viewed from outside the sphere.
            indices.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    Mesh { vertices, indices }
}

const MAJOR: [f32; 4] = [0.72, 0.80, 0.90, 0.30];
const REFERENCE: [f32; 4] = [0.93, 0.86, 0.68, 0.48];
const MINOR: [f32; 4] = [0.68, 0.76, 0.86, 0.16];

pub fn graticule(step_deg: f32, minor: bool) -> Mesh<LineVertex> {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let radius = EARTH_R * 1.0035;
    let steps = if minor { 64 } else { 128 };

    let mut lat = -90.0 + step_deg;
    while lat < 90.0 - step_deg * 0.25 {
        if !(minor && multiple_of(lat, 15.0)) {
            let color = if !minor && lat.abs() < 0.1 { REFERENCE } else if minor { MINOR } else { MAJOR };
            push_parallel(&mut vertices, &mut indices, lat.to_radians(), radius, steps, color);
        }
        lat += step_deg;
    }

    let mut lon = -180.0_f32;
    while lon < 180.0 - 0.1 {
        if !(minor && multiple_of(lon, 15.0)) {
            let color = if !minor && lon.abs() < 0.1 { REFERENCE } else if minor { MINOR } else { MAJOR };
            push_meridian(&mut vertices, &mut indices, lon.to_radians(), radius, steps, color);
        }
        lon += step_deg;
    }

    Mesh { vertices, indices }
}

fn multiple_of(value: f32, step: f32) -> bool {
    let q = (value / step).round();
    (value - q * step).abs() < 0.2
}

fn push_parallel(
    vertices: &mut Vec<LineVertex>,
    indices: &mut Vec<u32>,
    lat: f32,
    radius: f32,
    steps: u32,
    color: [f32; 4],
) {
    let base = vertices.len() as u32;
    for i in 0..=steps {
        let lon = -PI + std::f32::consts::TAU * (i as f32 / steps as f32);
        let p = geo::lat_lon_to_dir(lat, lon) * radius;
        vertices.push(LineVertex { position: p.to_array(), color });
        if i > 0 {
            indices.push(base + i - 1);
            indices.push(base + i);
        }
    }
}

fn push_meridian(
    vertices: &mut Vec<LineVertex>,
    indices: &mut Vec<u32>,
    lon: f32,
    radius: f32,
    steps: u32,
    color: [f32; 4],
) {
    let base = vertices.len() as u32;
    for i in 0..=steps {
        let lat = -PI * 0.5 + PI * (i as f32 / steps as f32);
        let p = geo::lat_lon_to_dir(lat, lon) * radius;
        vertices.push(LineVertex { position: p.to_array(), color });
        if i > 0 {
            indices.push(base + i - 1);
            indices.push(base + i);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    #[test]
    fn equator_prime_meridian_sits_on_plus_z_with_centered_uv() {
        let mesh = uv_sphere(4, 8, 1.0);
        let vertex = mesh
            .vertices
            .iter()
            .find(|v| Vec3::from(v.position).distance(Vec3::Z) < 1e-4)
            .expect("missing lon 0 / lat 0 vertex");
        assert!((vertex.uv[0] - 0.5).abs() < 1e-4, "u {}", vertex.uv[0]);
        assert!((vertex.uv[1] - 0.5).abs() < 1e-4, "v {}", vertex.uv[1]);
        let north = mesh
            .vertices
            .iter()
            .find(|v| v.position[1] > 0.99)
            .expect("north pole");
        assert!(north.uv[1] < 0.01, "north pole should be the top row, v {}", north.uv[1]);
    }

    #[test]
    fn triangles_face_outward() {
        let mesh = uv_sphere(12, 24, 1.0);
        let mut checked = 0u32;
        for tri in mesh.indices.chunks_exact(3) {
            let p0 = Vec3::from(mesh.vertices[tri[0] as usize].position);
            let p1 = Vec3::from(mesh.vertices[tri[1] as usize].position);
            let p2 = Vec3::from(mesh.vertices[tri[2] as usize].position);
            let n = (p1 - p0).cross(p2 - p0);
            if n.length() < 1e-5 {
                continue;
            }
            let centroid = (p0 + p1 + p2).normalize();
            assert!(
                n.normalize().dot(centroid) > 0.0,
                "triangle faces inward"
            );
            checked += 1;
        }
        assert!(checked > 100);
    }
}
