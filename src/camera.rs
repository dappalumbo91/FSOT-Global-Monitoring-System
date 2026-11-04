//! Google-Earth-style orbit camera.
//!
//! The focus is a point on the sphere. `height` is the distance from that
//! point to the eye, in Earth radii. Heading 0 looks north and increases
//! clockwise. Tilt 0 looks straight down the local up vector.
//!
//! East, north, and the geographic position all come from [`crate::geo`].
//! Screen axes are read back from the same eye / focus / up triple that builds
//! the view matrix.

use crate::geo::{self, EARTH_R};
use glam::{Mat4, Quat, Vec2, Vec3, Vec4};

pub const MIN_HEIGHT: f32 = 0.012;
pub const MAX_HEIGHT: f32 = 5.0;

#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub lat: f32,
    pub lon: f32,
    pub height: f32,
    pub heading: f32,
    pub tilt: f32,
    pub fov_y: f32,
}

impl Camera {
    pub fn new(lat_deg: f32, lon_deg: f32, height: f32) -> Self {
        Self {
            lat: lat_deg.to_radians(),
            lon: lon_deg.to_radians(),
            height,
            heading: 0.0,
            tilt: 0.0,
            fov_y: 40.0_f32.to_radians(),
        }
    }

    /// Day-side view for the given UTC instant, with the terminator in frame.
    pub fn sunlit(unix_secs: f64) -> Self {
        let (lat, lon) = geo::subsolar_lat_lon(unix_secs);
        let mut cam = Self {
            lat: ((lat as f32) * 0.45).clamp(-0.5, 0.6),
            lon: geo::wrap_pi(lon as f32 - 0.42),
            height: 3.05,
            heading: 0.0,
            tilt: 0.16,
            fov_y: 40.0_f32.to_radians(),
        };
        cam.tilt = cam.tilt.min(cam.max_tilt());
        cam
    }

    pub fn focus_dir(&self) -> Vec3 {
        geo::lat_lon_to_dir(self.lat, self.lon)
    }

    pub fn set_focus_dir(&mut self, dir: Vec3) {
        let (lat, lon) = geo::dir_to_lat_lon(dir);
        self.lat = lat;
        self.lon = lon;
    }

    pub fn max_tilt(&self) -> f32 {
        let geometric = (EARTH_R / (EARTH_R + self.height)).asin() * 0.96;
        let eased = (1.0 / (self.height + 0.2)).clamp(0.0, 1.0) * 1.15;
        geometric.min(eased)
    }

    /// Eye, surface focus, and the up hint handed to `look_at`.
    pub fn eye_target_up(&self) -> (Vec3, Vec3, Vec3) {
        let (east, north, up) = geo::enu(self.lat, self.lon);
        let heading = geo::wrap_pi(self.heading);
        let forward = north * heading.cos() + east * heading.sin();
        let tilt = self.tilt.clamp(0.0, self.max_tilt().max(self.tilt.min(1.2)));
        let focus = up * EARTH_R;
        let offset = up * tilt.cos() - forward * tilt.sin();
        let mut eye = focus + offset * self.height.max(MIN_HEIGHT);
        let min_r = EARTH_R + 0.004;
        if eye.length() < min_r {
            eye = eye.normalize() * min_r;
        }
        let up_hint = (up * tilt.sin() + forward * tilt.cos()).normalize();
        (eye, focus, up_hint)
    }

    pub fn eye(&self) -> Vec3 {
        self.eye_target_up().0
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        let (eye, focus, up) = self.eye_target_up();
        let near = (self.height * 0.05).clamp(0.0005, 0.15);
        let view = Mat4::look_at_rh(eye, focus, up);
        let proj = Mat4::perspective_rh(self.fov_y, aspect.max(0.01), near, 220.0);
        proj * view
    }

    /// Ray in physical pixels, origin at the top-left, y down.
    pub fn ray(&self, px: f32, py: f32, width: f32, height: f32) -> (Vec3, Vec3) {
        let inv = self.view_proj(width / height).inverse();
        let ndc_x = (px / width) * 2.0 - 1.0;
        let ndc_y = 1.0 - (py / height) * 2.0;
        let mut near = inv * Vec4::new(ndc_x, ndc_y, 0.0, 1.0);
        let mut far = inv * Vec4::new(ndc_x, ndc_y, 1.0, 1.0);
        near /= near.w;
        far /= far.w;
        let origin = near.truncate();
        let dir = (far.truncate() - origin).normalize();
        (origin, dir)
    }

    pub fn pick(&self, px: f32, py: f32, width: f32, height: f32) -> Option<Vec3> {
        let (origin, dir) = self.ray(px, py, width, height);
        let t = ray_sphere(origin, dir, EARTH_R)?;
        Some((origin + dir * t).normalize())
    }

    pub fn project(&self, world: Vec3, width: f32, height: f32) -> Option<Vec2> {
        let clip = self.view_proj(width / height) * world.extend(1.0);
        if clip.w <= 0.0 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        if !(0.0..=1.0).contains(&ndc.z) {
            return None;
        }
        Some(Vec2::new(
            (ndc.x * 0.5 + 0.5) * width,
            (1.0 - (ndc.y * 0.5 + 0.5)) * height,
        ))
    }

    /// Drag the surface point `prev` onto the screen location that currently
    /// hits `curr`. Focus moves opposite the pointer.
    pub fn grab(&mut self, prev: Vec3, curr: Vec3) {
        let prev = prev.normalize();
        let curr = curr.normalize();
        let dot = prev.dot(curr);
        if dot > 0.999_999 || dot < -0.99 {
            return;
        }
        let q = Quat::from_rotation_arc(curr, prev);
        self.set_focus_dir(q * self.focus_dir());
    }

    /// Pixel drag in physical pixels, y down. Used when the pointer misses the
    /// sphere and for inertia. A rightward drag moves the focus west when
    /// north is up; a downward drag moves the focus north.
    pub fn spin_pixels(&mut self, dx: f32, dy: f32, screen_h: f32) {
        if screen_h < 1.0 || (dx == 0.0 && dy == 0.0) {
            return;
        }
        let (eye, focus, up_hint) = self.eye_target_up();
        let view = (focus - eye).normalize();
        let right = view.cross(up_hint).normalize();
        let up = right.cross(view).normalize();
        let world_per_px = (2.0 * self.height * (self.fov_y * 0.5).tan()) / screen_h;
        let slant = self.tilt.cos().abs().max(0.35);
        let ang = world_per_px / EARTH_R / slant;
        let q = Quat::from_axis_angle(up, -dx * ang) * Quat::from_axis_angle(right, -dy * ang);
        self.set_focus_dir(q * self.focus_dir());
    }

    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.heading = geo::wrap_pi(self.heading + dx * 0.0045);
        self.tilt = (self.tilt + dy * 0.004).clamp(0.0, self.max_tilt());
    }

    pub fn zoom(&mut self, steps: f32, hit: Option<Vec3>) {
        let old = self.height;
        let new = (old * (-steps * 0.22).exp()).clamp(MIN_HEIGHT, MAX_HEIGHT);
        self.height = new;
        if let Some(hit) = hit {
            if new < old - f32::EPSILON {
                let t = ((old - new) / old).clamp(0.0, 1.0);
                let focus = self.focus_dir();
                self.set_focus_dir((focus * (1.0 - t) + hit.normalize() * t).normalize());
            }
        }
        self.tilt = self.tilt.min(self.max_tilt());
    }

    pub fn zoom_double(&mut self, hit: Option<Vec3>) {
        let old = self.height;
        self.height = (old * 0.55).clamp(MIN_HEIGHT, MAX_HEIGHT);
        if let Some(hit) = hit {
            let focus = self.focus_dir();
            self.set_focus_dir((focus * 0.55 + hit.normalize() * 0.45).normalize());
        }
        self.tilt = self.tilt.min(self.max_tilt());
    }

    pub fn ease_tilt(&mut self, dt: f32) {
        let max_t = self.max_tilt();
        if self.tilt > max_t {
            let k = 1.0 - (-5.0 * dt).exp();
            self.tilt += (max_t - self.tilt) * k;
        }
    }
}

fn ray_sphere(origin: Vec3, dir: Vec3, radius: f32) -> Option<f32> {
    let b = origin.dot(dir);
    let c = origin.dot(origin) - radius * radius;
    let disc = b * b - c;
    if disc < 0.0 {
        return None;
    }
    let s = disc.sqrt();
    let t0 = -b - s;
    let t1 = -b + s;
    if t0 > 1e-4 {
        Some(t0)
    } else if t1 > 1e-4 {
        Some(t1)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn equatorial() -> Camera {
        Camera::new(0.0, 0.0, 3.05)
    }

    #[test]
    fn center_pick_and_projection_agree() {
        for tilt in [0.0_f32, 0.2, 0.45] {
            let mut cam = equatorial();
            cam.tilt = tilt.min(cam.max_tilt());
            cam.heading = 0.3;
            let hit = cam.pick(720.0, 450.0, 1440.0, 900.0).expect("center hit");
            let ang = hit.dot(cam.focus_dir()).clamp(-1.0, 1.0).acos();
            assert!(
                ang < 0.5_f32.to_radians(),
                "tilt {tilt} center miss {:.3}°",
                ang.to_degrees()
            );
            let p = cam
                .project(cam.focus_dir() * EARTH_R, 1440.0, 900.0)
                .expect("focus projects");
            assert!((p.x - 720.0).abs() < 1.5, "x {}", p.x);
            assert!((p.y - 450.0).abs() < 1.5, "y {}", p.y);
        }
    }

    #[test]
    fn screen_axes_follow_the_geographic_frame() {
        let cam = equatorial();
        let north = cam
            .project(geo::lat_lon_to_dir(20.0_f32.to_radians(), 0.0), 1440.0, 900.0)
            .expect("north");
        let east = cam
            .project(geo::lat_lon_to_dir(0.0, 20.0_f32.to_radians()), 1440.0, 900.0)
            .expect("east");
        assert!(north.y < 450.0 - 8.0, "north should sit above center, y {}", north.y);
        assert!(east.x > 720.0 + 8.0, "east should sit right of center, x {}", east.x);
    }

    #[test]
    fn drag_moves_focus_opposite_the_pointer() {
        let mut cam = equatorial();
        let prev = cam.focus_dir();
        cam.grab(prev, geo::lat_lon_to_dir(0.0, 0.15));
        assert!(cam.lon < -0.05, "eastward grab should move focus west, lon {}", cam.lon);

        let mut cam = equatorial();
        cam.grab(cam.focus_dir(), geo::lat_lon_to_dir(-0.15, 0.0));
        assert!(cam.lat > 0.05, "southward grab should move focus north, lat {}", cam.lat);

        let mut cam = equatorial();
        let lon0 = cam.lon;
        cam.spin_pixels(40.0, 0.0, 900.0);
        assert!(
            cam.lon < lon0 - 1.0_f32.to_radians(),
            "right drag should decrease longitude"
        );
        let mut cam = equatorial();
        cam.spin_pixels(0.0, 40.0, 900.0);
        assert!(cam.lat > 1.0_f32.to_radians(), "down drag should increase latitude");
    }

    #[test]
    fn pole_camera_stays_finite() {
        let mut cam = Camera::new(89.0, 10.0, 1.2);
        cam.tilt = 0.3;
        let (eye, focus, up) = cam.eye_target_up();
        assert!(eye.is_finite() && focus.is_finite() && up.is_finite());
        let view = (focus - eye).normalize();
        assert!(up.dot(view).abs() < 0.05, "up parallel to view: {}", up.dot(view));
        let hit = cam.pick(720.0, 450.0, 1440.0, 900.0).expect("pole pick");
        let ang = hit.dot(cam.focus_dir()).clamp(-1.0, 1.0).acos();
        assert!(ang < 1.0_f32.to_radians(), "pole center miss {}°", ang.to_degrees());
    }

    #[test]
    fn zoom_in_pulls_focus_toward_the_cursor() {
        let mut cam = equatorial();
        let hit = geo::lat_lon_to_dir(0.2, 0.25);
        cam.zoom(2.0, Some(hit));
        assert!(cam.height < 3.05);
        let focus = cam.focus_dir();
        assert!(focus.dot(hit) > geo::lat_lon_to_dir(0.0, 0.0).dot(hit));
    }
}
