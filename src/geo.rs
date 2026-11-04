//! Earth frame shared by the mesh, the camera, the labels, and later layers.
//!
//! Y is up. The north pole is +Y. Latitude 0, longitude 0 faces +Z, and +X is
//! longitude +90° (east). Distances are Earth radii unless a function says
//! otherwise. One degree of latitude is the same physical step everywhere;
//! longitude is measured positive east of Greenwich.

use glam::Vec3;
use std::f32::consts::{PI, TAU};

pub const EARTH_R: f32 = 1.0;
pub const EARTH_RADIUS_KM: f32 = 6371.0;

pub fn lat_lon_to_dir(lat: f32, lon: f32) -> Vec3 {
    let (s_lat, c_lat) = lat.sin_cos();
    let (s_lon, c_lon) = lon.sin_cos();
    Vec3::new(c_lat * s_lon, s_lat, c_lat * c_lon)
}

pub fn dir_to_lat_lon(dir: Vec3) -> (f32, f32) {
    let d = dir.normalize();
    (d.y.clamp(-1.0, 1.0).asin(), d.x.atan2(d.z))
}

/// Equirectangular texture coordinate. `v = 0` is the north pole, the first
/// row of the NASA maps. `u = 0.5` is longitude 0.
pub fn equirect_uv(lat: f32, lon: f32) -> [f32; 2] {
    [(lon + PI) / TAU, (PI * 0.5 - lat) / PI]
}

/// Local east, north, and up. East is `d(position)/d(longitude)`, which stays
/// a unit vector at the poles. North is the direction of increasing latitude.
pub fn enu(lat: f32, lon: f32) -> (Vec3, Vec3, Vec3) {
    let up = lat_lon_to_dir(lat, lon);
    let east = Vec3::new(lon.cos(), 0.0, -lon.sin());
    let north = up.cross(east).normalize();
    (east, north, up)
}

pub fn wrap_pi(radians: f32) -> f32 {
    (radians + PI).rem_euclid(TAU) - PI
}

pub fn format_lat_lon_deg(lat_deg: f32, lon_deg: f32) -> String {
    let ns = if lat_deg >= 0.0 { "N" } else { "S" };
    let ew = if lon_deg >= 0.0 { "E" } else { "W" };
    format!("{:.2}° {ns}  {:.2}° {ew}", lat_deg.abs(), lon_deg.abs())
}

pub fn unix_now() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Days since 1970-01-01. Howard Hinnant's civil-from-days conversion.
pub fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let mut y = i64::from(y);
    let m = i64::from(m);
    let d = i64::from(d);
    y -= if m <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy as u64;
    era * 146097 + doe as i64 - 719468
}

pub fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}

pub fn unix_utc(year: i32, month: u32, day: u32, hour: f64) -> f64 {
    days_from_civil(year, month, day) as f64 * 86400.0 + hour * 3600.0
}

pub fn format_unix_utc(unix_secs: f64) -> String {
    let secs = unix_secs.floor() as i64;
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (y, m, d) = civil_from_days(days);
    let h = rem / 3600;
    let min = (rem % 3600) / 60;
    let s = rem % 60;
    format!("{y:04}-{m:02}-{d:02}  {h:02}:{min:02}:{s:02} UTC")
}

/// Subsolar latitude and longitude in radians. Low-precision NOAA / Meeus
/// solar position, accurate to well under a degree, which is enough to place
/// the terminator.
pub fn subsolar_lat_lon(unix_secs: f64) -> (f64, f64) {
    let jd = unix_secs / 86400.0 + 2440587.5;
    let n = jd - 2451545.0;
    let mean_anom = (357.52911 + 0.98560028 * n).rem_euclid(360.0).to_radians();
    let mean_long = 280.459 + 0.98564736 * n;
    // 1.915° and 0.020° are the equation-of-center coefficients. The sines stay
    // dimensionless; converting them to degrees shifts the June declination by ~2°.
    let lambda = (mean_long
        + 1.915 * mean_anom.sin()
        + 0.020 * (2.0 * mean_anom).sin())
    .rem_euclid(360.0)
    .to_radians();
    let obliquity = (23.439291 - 0.00000036 * n).to_radians();
    let ra = (lambda.sin() * obliquity.cos()).atan2(lambda.cos());
    let dec = (lambda.sin() * obliquity.sin()).asin();
    let century = n / 36525.0;
    let gmst = (280.46061837 + 360.98564736629 * n + 0.000387933 * century * century)
        .rem_euclid(360.0)
        .to_radians();
    let lon = (ra - gmst + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)
        - std::f64::consts::PI;
    (dec, lon)
}

pub fn sun_direction(unix_secs: f64) -> Vec3 {
    let (lat, lon) = subsolar_lat_lon(unix_secs);
    lat_lon_to_dir(lat as f32, lon as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::FRAC_PI_2;

    #[test]
    fn axes_match_the_documented_frame() {
        assert!(lat_lon_to_dir(0.0, 0.0).distance(Vec3::Z) < 1e-5);
        assert!(lat_lon_to_dir(0.0, FRAC_PI_2).distance(Vec3::X) < 1e-5);
        assert!(lat_lon_to_dir(0.0, -FRAC_PI_2).distance(Vec3::NEG_X) < 1e-5);
        assert!(lat_lon_to_dir(FRAC_PI_2, 0.4).distance(Vec3::Y) < 1e-5);
        let uv = equirect_uv(0.0, 0.0);
        assert!((uv[0] - 0.5).abs() < 1e-6 && (uv[1] - 0.5).abs() < 1e-6);
        let north_uv = equirect_uv(FRAC_PI_2, 0.2);
        assert!(north_uv[1].abs() < 1e-6);
        let east_uv = equirect_uv(0.0, FRAC_PI_2);
        assert!((east_uv[0] - 0.75).abs() < 1e-5);
    }

    #[test]
    fn lat_lon_roundtrip() {
        for lat_d in [-80.0_f32, -20.0, 0.0, 15.0, 60.0, 89.0] {
            for lon_d in [-180.0_f32, -90.0, -20.0, 0.0, 40.0, 179.0] {
                let lat = lat_d.to_radians();
                let lon = lon_d.to_radians();
                let (lat2, lon2) = dir_to_lat_lon(lat_lon_to_dir(lat, lon));
                assert!((lat2 - lat).abs() < 2e-4, "lat {lat_d} -> {lat2}");
                assert!(wrap_pi(lon2 - lon).abs() < 2e-3, "lon {lon_d} -> {lon2}");
            }
        }
    }

    #[test]
    fn north_increases_latitude() {
        for (lat_d, lon_d) in [(0.0_f32, 0.0_f32), (35.0, 90.0), (89.0, 20.0), (-40.0, -120.0)] {
            let lat = lat_d.to_radians();
            let lon = lon_d.to_radians();
            let (east, north, up) = enu(lat, lon);
            assert!((east.length() - 1.0).abs() < 1e-5);
            assert!(east.dot(up).abs() < 1e-4, "east not perpendicular at {lat_d},{lon_d}");
            assert!(north.dot(up).abs() < 1e-4);
            assert!(east.dot(north).abs() < 1e-4);
            let (lat2, _) = dir_to_lat_lon(up + north * 0.02);
            assert!(lat2 > lat, "north did not increase latitude at {lat_d},{lon_d}");
        }
        let (east, north, up) = enu(0.0, 0.0);
        assert!(east.distance(Vec3::X) < 1e-4);
        assert!(north.distance(Vec3::Y) < 1e-4);
        assert!(up.distance(Vec3::Z) < 1e-4);
    }

    #[test]
    fn civil_dates_roundtrip() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        for (y, m, d) in [(2024, 2, 29), (2026, 10, 31), (1999, 12, 31), (1969, 7, 20)] {
            let (yy, mm, dd) = civil_from_days(days_from_civil(y, m, d));
            assert_eq!((yy, mm, dd), (y, m, d));
        }
        assert_eq!(
            format_unix_utc(unix_utc(2026, 10, 31, 5.0)),
            "2026-10-31  05:00:00 UTC"
        );
    }

    #[test]
    fn sun_tracks_the_seasons_and_greenwich() {
        let (jun, _) = subsolar_lat_lon(unix_utc(2024, 6, 21, 12.0));
        assert!(
            (22.0..24.6).contains(&jun.to_degrees()),
            "june declination {}",
            jun.to_degrees()
        );
        let (dec, _) = subsolar_lat_lon(unix_utc(2024, 12, 21, 12.0));
        assert!(
            (-24.6..-22.0).contains(&dec.to_degrees()),
            "december declination {}",
            dec.to_degrees()
        );
        let (eq_lat, eq_lon) = subsolar_lat_lon(unix_utc(2024, 3, 20, 12.0));
        assert!(
            eq_lat.to_degrees().abs() < 4.0,
            "equinox declination {}",
            eq_lat.to_degrees()
        );
        assert!(
            eq_lon.to_degrees().abs() < 12.0,
            "equinox subsolar longitude {}",
            eq_lon.to_degrees()
        );
        let sun = sun_direction(unix_utc(2024, 6, 21, 12.0));
        assert!((sun.length() - 1.0).abs() < 1e-4);
    }

    #[test]
    fn coordinate_readout_format() {
        assert_eq!(format_lat_lon_deg(18.0, -20.5), "18.00° N  20.50° W");
        assert_eq!(format_lat_lon_deg(-5.5, 100.0), "5.50° S  100.00° E");
    }
}
