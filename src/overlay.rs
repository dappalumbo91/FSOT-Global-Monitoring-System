//! Markers, fault lines, wind streaks, and the elevation tint.
//! Positions go through [`crate::geo`], same as the globe mesh.

use crate::camera::Camera;
use crate::feeds::{self, City, ElevationGrid, OceanSample, Quake, Storm, Volcano, VolcanoAlert, WeeklyVolcano, WindSample};
use crate::geo::{self, EARTH_R};
use crate::mesh::{LineVertex, Mesh};
use glam::Vec3;
use std::f32::consts::TAU;

pub struct DynLabel {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub alpha: f32,
    pub rgb: [u8; 3],
}

const FAULT: [f32; 4] = [0.95, 0.42, 0.28, 0.72];

pub fn fault_lines(chains: &[Vec<(f32, f32)>]) -> Mesh<LineVertex> {
    let mut mesh = Mesh { vertices: Vec::new(), indices: Vec::new() };
    for chain in chains {
        for pair in chain.windows(2) {
            push_arc(&mut mesh, pair[0].0, pair[0].1, pair[1].0, pair[1].1, EARTH_R * 1.005, FAULT);
        }
    }
    mesh
}

pub fn volcano_markers(volcanoes: &[Volcano], alerts: &[VolcanoAlert]) -> Mesh<LineVertex> {
    let mut mesh = Mesh { vertices: Vec::new(), indices: Vec::new() };
    for volcano in volcanoes {
        if let Some(alert) = matching_alert(alerts, volcano.lat, volcano.lon) {
            push_marker(&mut mesh, volcano.lat, volcano.lon, EARTH_R * 1.012, 110.0, alert_color(&alert.color_code));
            continue;
        }
        let recent = volcano.last_year.unwrap_or(i32::MIN) >= 2000;
        let color = if recent {
            [1.0, 0.42, 0.12, 0.95]
        } else {
            [0.72, 0.22, 0.16, 0.55]
        };
        let km = if recent { 70.0 } else { 40.0 };
        push_marker(&mut mesh, volcano.lat, volcano.lon, EARTH_R * 1.008, km, color);
    }
    for alert in alerts {
        if volcanoes.iter().any(|volcano| feeds::km_between(volcano.lat, volcano.lon, alert.lat, alert.lon) <= 80.0) {
            continue;
        }
        push_marker(&mut mesh, alert.lat, alert.lon, EARTH_R * 1.012, 110.0, alert_color(&alert.color_code));
    }
    mesh
}

fn matching_alert<'a>(alerts: &'a [VolcanoAlert], lat: f32, lon: f32) -> Option<&'a VolcanoAlert> {
    alerts.iter().find(|alert| feeds::km_between(lat, lon, alert.lat, alert.lon) <= 80.0)
}

fn alert_color(code: &str) -> [f32; 4] {
    match code {
        "RED" => [1.0, 0.15, 0.1, 0.98],
        "ORANGE" => [1.0, 0.45, 0.08, 0.98],
        "YELLOW" => [1.0, 0.86, 0.2, 0.95],
        _ => [1.0, 0.95, 0.9, 0.9],
    }
}

pub fn quake_markers(quakes: &[Quake]) -> Mesh<LineVertex> {
    let mut mesh = Mesh { vertices: Vec::new(), indices: Vec::new() };
    for quake in quakes {
        let mag = quake.mag.max(0.0);
        let km = 28.0 + mag * 16.0;
        let color = if mag >= 6.0 {
            [1.0, 0.95, 0.85, 0.95]
        } else if mag >= 5.0 {
            [1.0, 0.28, 0.18, 0.92]
        } else if mag >= 4.0 {
            [1.0, 0.55, 0.16, 0.88]
        } else {
            [1.0, 0.82, 0.28, 0.75]
        };
        push_marker(&mut mesh, quake.lat, quake.lon, EARTH_R * 1.01, km, color);
    }
    mesh
}

pub fn append_mesh(into: &mut Mesh<LineVertex>, extra: Mesh<LineVertex>) {
    let base = into.vertices.len() as u32;
    into.vertices.extend(extra.vertices);
    into.indices.extend(extra.indices.into_iter().map(|index| index + base));
}

pub fn weekly_markers(reports: &[WeeklyVolcano]) -> Mesh<LineVertex> {
    let mut mesh = Mesh { vertices: Vec::new(), indices: Vec::new() };
    for report in reports {
        if !report.kind.to_ascii_lowercase().starts_with("new") {
            continue;
        }
        push_marker(&mut mesh, report.lat, report.lon, EARTH_R * 1.013, 96.0, [0.72, 0.38, 0.95, 0.95]);
    }
    mesh
}

pub fn storm_markers(storms: &[Storm]) -> Mesh<LineVertex> {
    let mut mesh = Mesh { vertices: Vec::new(), indices: Vec::new() };
    for storm in storms {
        push_marker(&mut mesh, storm.lat, storm.lon, EARTH_R * 1.014, 170.0, [1.0, 0.96, 0.82, 0.95]);
    }
    mesh
}

pub fn ocean_markers(samples: &[OceanSample]) -> Mesh<LineVertex> {
    let mut mesh = Mesh { vertices: Vec::new(), indices: Vec::new() };
    for sample in samples {
        let t = ((sample.sst_c + 2.0) / 32.0).clamp(0.0, 1.0);
        let color = [0.15 + 0.80 * t, 0.38 + 0.32 * t, 0.86 - 0.66 * t, 0.84];
        let km = 140.0 + sample.wave_m.clamp(0.0, 12.0) * 28.0;
        push_marker(&mut mesh, sample.lat, sample.lon, EARTH_R * 1.007, km, color);
    }
    mesh
}

/// Hypsometric tint. RGB is the land color, A is height scaled to 6 km.
/// Ocean and missing samples stay clear so the imagery shows through.
pub fn elevation_rgba(grid: &ElevationGrid) -> Option<(u32, u32, Vec<u8>)> {
    if grid.nlat < 2 || grid.nlon < 2 || grid.meters.len() < grid.nlat * grid.nlon {
        return None;
    }
    let width = grid.nlon as u32;
    let height = grid.nlat as u32;
    let mut rgba = vec![0u8; grid.nlat * grid.nlon * 4];
    for (i, meters) in grid.meters.iter().enumerate() {
        if *meters <= 20 || *meters <= -30_000 {
            continue;
        }
        let color = elevation_color(*meters as f32);
        let scaled = (*meters as f32 / 6000.0).clamp(0.0, 1.0);
        let o = i * 4;
        rgba[o] = (color[0] * 255.0) as u8;
        rgba[o + 1] = (color[1] * 255.0) as u8;
        rgba[o + 2] = (color[2] * 255.0) as u8;
        rgba[o + 3] = (scaled * 255.0) as u8;
    }
    Some((width, height, rgba))
}

/// Wind streaks move with the measured wind. `from_deg` is the direction the
/// wind comes from, the same convention Open-Meteo publishes.
pub fn wind_streaks(samples: &[WindSample], unix: f64) -> Mesh<LineVertex> {
    let mut mesh = Mesh { vertices: Vec::new(), indices: Vec::new() };
    let radius = EARTH_R * 1.012;
    for sample in samples {
        let speed = sample.wind_ms.max(0.0);
        if speed < 0.4 {
            continue;
        }
        let dir = wind_motion_dir(sample.lat, sample.lon, sample.from_deg);
        if dir.length_squared() < 1e-8 {
            continue;
        }
        let dir = dir.normalize();
        let span = (0.04 + speed * 0.005).clamp(0.04, 0.14);
        let cycle = (unix * (0.04 + f64::from(speed) * 0.01) + f64::from(sample.lat + sample.lon) * 0.01).rem_euclid(1.0) as f32;
        let along = (cycle - 0.5) * span;
        let mid = geo::lat_lon_to_dir(sample.lat.to_radians(), sample.lon.to_radians());
        let start = (mid + dir * (along - span * 0.35)).normalize() * radius;
        let end = (mid + dir * (along + span * 0.35)).normalize() * radius;
        let t = (speed / 18.0).clamp(0.0, 1.0);
        let color = [0.62 + 0.34 * t, 0.9, 0.98 - 0.22 * t, 0.62 + 0.34 * t];
        let base = mesh.vertices.len() as u32;
        mesh.vertices.push(LineVertex { position: start.to_array(), color });
        mesh.vertices.push(LineVertex { position: end.to_array(), color });
        mesh.indices.extend_from_slice(&[base, base + 1]);
        let side = dir.cross(mid);
        if side.length_squared() > 1e-8 {
            let side = side.normalize();
            let back = (end / radius - dir * span * 0.28).normalize();
            let wing = span * 0.16;
            let left = (back + side * wing).normalize() * radius;
            let right = (back - side * wing).normalize() * radius;
            let tip = mesh.vertices.len() as u32;
            mesh.vertices.push(LineVertex { position: end.to_array(), color });
            mesh.vertices.push(LineVertex { position: left.to_array(), color });
            mesh.vertices.push(LineVertex { position: right.to_array(), color });
            mesh.indices.extend_from_slice(&[tip, tip + 1, tip, tip + 2]);
        }
    }
    mesh
}

pub fn wind_motion_dir(lat_deg: f32, lon_deg: f32, from_deg: f32) -> Vec3 {
    let (east, north, _) = geo::enu(lat_deg.to_radians(), lon_deg.to_radians());
    let from = from_deg.to_radians();
    -north * from.cos() - east * from.sin()
}

/// Ocean current points where the water is going. Open-Meteo: 0° goes north, 90° goes east.
pub fn current_toward_dir(lat_deg: f32, lon_deg: f32, toward_deg: f32) -> Vec3 {
    let (east, north, _) = geo::enu(lat_deg.to_radians(), lon_deg.to_radians());
    let toward = toward_deg.to_radians();
    north * toward.cos() + east * toward.sin()
}

pub fn current_streaks(samples: &[OceanSample]) -> Mesh<LineVertex> {
    let mut mesh = Mesh { vertices: Vec::new(), indices: Vec::new() };
    let radius = EARTH_R * 1.009;
    for sample in samples {
        let Some(speed) = sample.current_kmh else { continue };
        let Some(toward) = sample.current_toward_deg else { continue };
        if speed < 0.3 {
            continue;
        }
        let dir = current_toward_dir(sample.lat, sample.lon, toward);
        if dir.length_squared() < 1e-8 {
            continue;
        }
        let dir = dir.normalize();
        let span = (0.03 + speed * 0.004).clamp(0.03, 0.12);
        let mid = geo::lat_lon_to_dir(sample.lat.to_radians(), sample.lon.to_radians());
        let start = (mid - dir * span * 0.45).normalize() * radius;
        let end = (mid + dir * span * 0.45).normalize() * radius;
        let t = (speed / 8.0).clamp(0.0, 1.0);
        let color = [0.2, 0.72 + 0.2 * t, 0.38, 0.55 + 0.35 * t];
        let base = mesh.vertices.len() as u32;
        mesh.vertices.push(LineVertex { position: start.to_array(), color });
        mesh.vertices.push(LineVertex { position: end.to_array(), color });
        mesh.indices.extend_from_slice(&[base, base + 1]);
        let side = dir.cross(mid);
        if side.length_squared() > 1e-8 {
            let side = side.normalize();
            let back = (end / radius - dir * span * 0.28).normalize();
            let wing = span * 0.16;
            let left = (back + side * wing).normalize() * radius;
            let right = (back - side * wing).normalize() * radius;
            let tip = mesh.vertices.len() as u32;
            mesh.vertices.push(LineVertex { position: end.to_array(), color });
            mesh.vertices.push(LineVertex { position: left.to_array(), color });
            mesh.vertices.push(LineVertex { position: right.to_array(), color });
            mesh.indices.extend_from_slice(&[tip, tip + 1, tip, tip + 2]);
        }
    }
    mesh
}

pub fn project_feed_labels(
    camera: &Camera,
    width: f32,
    height: f32,
    cities: &[City],
    quakes: &[Quake],
    volcanoes: &[Volcano],
    alerts: &[VolcanoAlert],
    storms: &[Storm],
    weekly: &[WeeklyVolcano],
    show_cities: bool,
    show_quakes: bool,
    show_volcanoes: bool,
    show_weather: bool,
) -> Vec<DynLabel> {
    let eye = camera.eye();
    let mut out = Vec::new();
    if show_cities {
        let min_pop = if camera.height > 1.15 {
            5_000_000
        } else if camera.height > 0.45 {
            1_000_000
        } else {
            200_000
        };
        for city in cities {
            if city.pop < min_pop {
                continue;
            }
            push_name(&mut out, camera, eye, width, height, city.lat, city.lon, &city.name, 13.0, [236, 232, 214]);
        }
    }
    if show_quakes {
        let min_mag = if camera.height > 0.7 { 5.5 } else { 4.5 };
        for quake in quakes {
            if quake.mag < min_mag {
                continue;
            }
            let text = format!("M{:.1}", quake.mag);
            push_name(&mut out, camera, eye, width, height, quake.lat, quake.lon, &text, 12.5, [255, 214, 170]);
        }
    }
    if show_volcanoes && camera.height < 0.32 {
        let focus = camera.focus_dir();
        for volcano in volcanoes {
            if matching_alert(alerts, volcano.lat, volcano.lon).is_some() {
                continue;
            }
            let dir = geo::lat_lon_to_dir(volcano.lat.to_radians(), volcano.lon.to_radians());
            if dir.dot(focus) < 0.95 {
                continue;
            }
            push_name(&mut out, camera, eye, width, height, volcano.lat, volcano.lon, &volcano.name, 12.0, [255, 186, 150]);
        }
    }
    if show_weather {
        for storm in storms {
            let name = if storm.name.is_empty() { storm.id.as_str() } else { storm.name.as_str() };
            push_name(&mut out, camera, eye, width, height, storm.lat, storm.lon, name, 15.0, [255, 244, 214]);
        }
    }
    if show_volcanoes {
        for report in weekly {
            if !report.kind.to_ascii_lowercase().starts_with("new") {
                continue;
            }
            if matching_alert(alerts, report.lat, report.lon).is_some() {
                continue;
            }
            push_name(&mut out, camera, eye, width, height, report.lat, report.lon, &report.name, 13.5, [226, 190, 255]);
        }
    }
    if show_volcanoes {
        for alert in alerts {
            let text = format!("{} {}", alert.name, alert.alert_level);
            let rgb = match alert.color_code.as_str() {
                "RED" => [255, 140, 120],
                "ORANGE" => [255, 176, 96],
                "YELLOW" => [255, 220, 120],
                _ => [255, 236, 220],
            };
            push_name(&mut out, camera, eye, width, height, alert.lat, alert.lon, &text, 14.0, rgb);
        }
    }
    out
}

fn elevation_color(meters: f32) -> [f32; 4] {
    let (r, g, b) = if meters < 200.0 {
        (0.22, 0.48, 0.24)
    } else if meters < 800.0 {
        (0.55, 0.58, 0.22)
    } else if meters < 2000.0 {
        (0.55, 0.36, 0.2)
    } else if meters < 4000.0 {
        (0.62, 0.58, 0.52)
    } else {
        (0.9, 0.92, 0.94)
    };
    [r, g, b, 0.32]
}

fn push_marker(mesh: &mut Mesh<LineVertex>, lat_deg: f32, lon_deg: f32, radius: f32, km: f32, color: [f32; 4]) {
    let lat = lat_deg.to_radians();
    let lon = lon_deg.to_radians();
    let (east, north, up) = geo::enu(lat, lon);
    let center = up * radius;
    let ang = (km / geo::EARTH_RADIUS_KM).clamp(0.0012, 0.08);
    let base = mesh.vertices.len() as u32;
    mesh.vertices.push(LineVertex { position: center.to_array(), color });
    let sides = 20u32;
    for i in 0..sides {
        let a = i as f32 / sides as f32 * TAU;
        let p = (center + (east * a.sin() + north * a.cos()) * ang).normalize() * radius;
        mesh.vertices.push(LineVertex { position: p.to_array(), color });
    }
    for i in 0..sides {
        let a = base + 1 + i;
        let b = base + 1 + (i + 1) % sides;
        mesh.indices.extend_from_slice(&[base, a, b]);
    }
}

fn push_arc(mesh: &mut Mesh<LineVertex>, lat0: f32, lon0: f32, lat1: f32, lon1: f32, radius: f32, color: [f32; 4]) {
    let a = geo::lat_lon_to_dir(lat0.to_radians(), lon0.to_radians());
    let b = geo::lat_lon_to_dir(lat1.to_radians(), lon1.to_radians());
    let ang = a.dot(b).clamp(-1.0, 1.0).acos();
    if ang < 1e-5 {
        return;
    }
    let steps = if ang < 3.0_f32.to_radians() {
        1
    } else {
        (ang.to_degrees() / 3.0).ceil() as u32
    };
    let mut prev = a;
    for step in 1..=steps {
        let t = step as f32 / steps as f32;
        let next = slerp(a, b, t);
        let base = mesh.vertices.len() as u32;
        mesh.vertices.push(LineVertex { position: (prev * radius).to_array(), color });
        mesh.vertices.push(LineVertex { position: (next * radius).to_array(), color });
        mesh.indices.extend_from_slice(&[base, base + 1]);
        prev = next;
    }
}

fn slerp(a: Vec3, b: Vec3, t: f32) -> Vec3 {
    let ang = a.dot(b).clamp(-1.0, 1.0).acos();
    if ang < 1e-5 {
        return a;
    }
    let s = ang.sin();
    ((a * ((1.0 - t) * ang).sin() + b * (t * ang).sin()) / s).normalize()
}

fn push_name(
    out: &mut Vec<DynLabel>,
    camera: &Camera,
    eye: Vec3,
    width: f32,
    height: f32,
    lat: f32,
    lon: f32,
    text: &str,
    size: f32,
    rgb: [u8; 3],
) {
    let dir = geo::lat_lon_to_dir(lat.to_radians(), lon.to_radians());
    let facing = eye.dot(dir);
    let start = 1.02;
    let end = 1.02 + (eye.length() - EARTH_R) * 0.12;
    let alpha = ((facing - start) / (end - start)).clamp(0.0, 1.0);
    if alpha < 0.08 {
        return;
    }
    let Some(p) = camera.project(dir * EARTH_R * 1.01, width, height) else {
        return;
    };
    if p.x < -20.0 || p.y < -12.0 || p.x > width + 20.0 || p.y > height + 12.0 {
        return;
    }
    out.push(DynLabel {
        text: text.to_string(),
        x: p.x,
        y: p.y - 12.0,
        size,
        alpha,
        rgb,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wind_from_north_blows_south_on_the_equator() {
        let dir = wind_motion_dir(0.0, 0.0, 0.0);
        assert!(dir.dot(Vec3::Y) < -0.9, "from north should move south, {dir}");
        let from_east = wind_motion_dir(0.0, 0.0, 90.0);
        assert!(from_east.dot(Vec3::X) < -0.9, "from east should move west, {from_east}");
    }

    #[test]
    fn current_toward_east_moves_east_on_the_equator() {
        let dir = current_toward_dir(0.0, 0.0, 90.0);
        assert!(dir.dot(Vec3::X) > 0.9, "toward east should move east, {dir}");
        let north = current_toward_dir(0.0, 0.0, 0.0);
        assert!(north.dot(Vec3::Y) > 0.9, "toward north should move north, {north}");
    }

    #[test]
    fn arc_stays_on_the_sphere() {
        let mesh = {
            let mut mesh = Mesh { vertices: Vec::new(), indices: Vec::new() };
            push_arc(&mut mesh, 10.0, 20.0, 12.0, 48.0, 1.005, FAULT);
            mesh
        };
        assert!(mesh.vertices.len() >= 2);
        for vertex in &mesh.vertices {
            let p = Vec3::from(vertex.position);
            assert!((p.length() - 1.005).abs() < 1e-3, "{}", p.length());
        }
    }

    #[test]
    fn elevation_tint_is_land_only() {
        let path = crate::renderer::project_path("assets/elevation_5deg.bin");
        if !path.exists() {
            return;
        }
        let grid = ElevationGrid::load(&path).expect("grid");
        let (width, height, rgba) = elevation_rgba(&grid).expect("rgba");
        assert_eq!((width, height), (72, 37));
        let at = |row: usize, col: usize| rgba[(row * 72 + col) * 4 + 3];
        let tibet_row = ((90.0_f32 - 30.0) / 5.0).round() as usize;
        let tibet_col = ((85.0_f32 + 180.0) / 5.0).round() as usize;
        let (lat, lon) = grid.lat_lon(tibet_row, tibet_col);
        assert!((lat - 30.0).abs() < 0.1 && (lon - 85.0).abs() < 0.1, "{lat} {lon}");
        assert!(at(tibet_row, tibet_col) > 100, "Tibet height byte");
        let ocean_row = (90.0_f32 / 5.0).round() as usize;
        let ocean_col = (180.0_f32 / 5.0).round() as usize;
        assert_eq!(at(ocean_row, ocean_col), 0, "equator ocean stays clear");
    }
}
