//! Continent, ocean, and sea names projected with the globe camera.
//! Screen positions are in physical pixels, origin top-left, y down.

use crate::camera::Camera;
use crate::geo::{self, EARTH_R};
use glam::Vec3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelKind {
    Continent,
    Ocean,
    Sea,
}

pub struct PlaceLabel {
    pub name: &'static str,
    pub kind: LabelKind,
    pub lat: f32,
    pub lon: f32,
}

pub struct ScreenLabel {
    pub name: &'static str,
    pub kind: LabelKind,
    pub x: f32,
    pub y: f32,
    pub alpha: f32,
    pub size: f32,
}

const PLACES: &[PlaceLabel] = &[
    PlaceLabel { name: "North America", kind: LabelKind::Continent, lat: 46.0, lon: -100.0 },
    PlaceLabel { name: "South America", kind: LabelKind::Continent, lat: -15.0, lon: -60.0 },
    PlaceLabel { name: "Europe", kind: LabelKind::Continent, lat: 50.0, lon: 10.0 },
    PlaceLabel { name: "Africa", kind: LabelKind::Continent, lat: 2.0, lon: 18.0 },
    PlaceLabel { name: "Asia", kind: LabelKind::Continent, lat: 42.0, lon: 90.0 },
    PlaceLabel { name: "Australia", kind: LabelKind::Continent, lat: -25.0, lon: 134.0 },
    PlaceLabel { name: "Antarctica", kind: LabelKind::Continent, lat: -78.0, lon: 0.0 },
    PlaceLabel { name: "Pacific Ocean", kind: LabelKind::Ocean, lat: 0.0, lon: -160.0 },
    PlaceLabel { name: "Atlantic Ocean", kind: LabelKind::Ocean, lat: 5.0, lon: -30.0 },
    PlaceLabel { name: "Indian Ocean", kind: LabelKind::Ocean, lat: -20.0, lon: 80.0 },
    PlaceLabel { name: "Arctic Ocean", kind: LabelKind::Ocean, lat: 78.0, lon: 0.0 },
    PlaceLabel { name: "Southern Ocean", kind: LabelKind::Ocean, lat: -58.0, lon: 80.0 },
    PlaceLabel { name: "Mediterranean Sea", kind: LabelKind::Sea, lat: 36.0, lon: 18.0 },
    PlaceLabel { name: "Caribbean Sea", kind: LabelKind::Sea, lat: 15.0, lon: -75.0 },
    PlaceLabel { name: "Gulf of Mexico", kind: LabelKind::Sea, lat: 25.0, lon: -90.0 },
    PlaceLabel { name: "South China Sea", kind: LabelKind::Sea, lat: 12.0, lon: 114.0 },
    PlaceLabel { name: "Arabian Sea", kind: LabelKind::Sea, lat: 15.0, lon: 64.0 },
    PlaceLabel { name: "Sea of Japan", kind: LabelKind::Sea, lat: 40.0, lon: 135.0 },
    PlaceLabel { name: "Coral Sea", kind: LabelKind::Sea, lat: -16.0, lon: 155.0 },
];

pub fn project_labels(camera: &Camera, width: f32, height: f32) -> Vec<ScreenLabel> {
    let eye = camera.eye();
    let closeness = (0.9 + 0.25 / camera.height.max(0.05)).clamp(0.85, 1.35);
    let mut out = Vec::new();
    for place in PLACES {
        let zoom = zoom_alpha(place.kind, camera.height);
        if zoom < 0.04 {
            continue;
        }
        let dir = geo::lat_lon_to_dir(place.lat.to_radians(), place.lon.to_radians());
        let facing = facing_alpha(eye, dir);
        let alpha = facing * zoom;
        if alpha < 0.04 {
            continue;
        }
        let Some(p) = camera.project(dir * EARTH_R * 1.002, width, height) else {
            continue;
        };
        if p.x < -40.0 || p.y < -20.0 || p.x > width + 40.0 || p.y > height + 20.0 {
            continue;
        }
        let base = match place.kind {
            LabelKind::Continent => 18.5,
            LabelKind::Ocean => 14.5,
            LabelKind::Sea => 13.0,
        };
        out.push(ScreenLabel {
            name: place.name,
            kind: place.kind,
            x: p.x,
            y: p.y,
            alpha,
            size: base * closeness,
        });
    }
    out
}

fn zoom_alpha(kind: LabelKind, height: f32) -> f32 {
    match kind {
        LabelKind::Continent => ((height - 0.08) / 0.12).clamp(0.0, 1.0),
        LabelKind::Ocean => ((height - 0.2) / 0.18).clamp(0.0, 1.0),
        LabelKind::Sea => {
            let fade_in = ((1.65 - height) / 0.45).clamp(0.0, 1.0);
            let fade_out = ((height - 0.16) / 0.14).clamp(0.0, 1.0);
            fade_in * fade_out
        }
    }
}

/// 0 on the hidden side of the limb, 1 once the point is clearly facing the eye.
fn facing_alpha(eye: Vec3, dir: Vec3) -> f32 {
    let d = eye.dot(dir);
    let start = 1.01;
    let end = 1.01 + (eye.length() - EARTH_R) * 0.16;
    ((d - start) / (end - start)).clamp(0.0, 1.0)
}
