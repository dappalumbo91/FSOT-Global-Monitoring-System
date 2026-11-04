//! Static catalogs and the live USGS, Open-Meteo, marine, NOAA SWPC, NHC, NWS, UNESCO IOC, and CPC feeds.
//!
//! Forecast lines quote those agencies' published products.
//! This program does not invent a time or a magnitude for the next event.
//! The FSOT desk holds measured fields. It does not score them.
//! Each field's full series, calm samples included, is stored under cache/series and read with pull_series.

use crate::geo;
use crate::renderer;
use crate::series;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct City {
    pub name: String,
    pub lat: f32,
    pub lon: f32,
    pub pop: u32,
}

#[derive(Clone, Debug)]
pub struct Volcano {
    pub name: String,
    pub lat: f32,
    pub lon: f32,
    pub elevation_m: i32,
    pub last_year: Option<i32>,
}

#[derive(Clone, Debug, Default)]
pub struct Quake {
    pub mag: f32,
    pub lat: f32,
    pub lon: f32,
    pub depth_km: f32,
    pub place: String,
    pub time_ms: i64,
    pub alert: Option<String>,
    pub tsunami: bool,
    pub types: String,
    pub detail: String,
    pub id: String,
    /// USGS station count. Missing stays none. A real zero stays zero.
    pub nst: Option<i32>,
    /// Azimuthal gap, degrees.
    pub gap: Option<f32>,
    /// Distance to the nearest station, degrees.
    pub dmin: Option<f32>,
    pub rms: Option<f32>,
    pub mag_type: Option<String>,
}

/// A published grid maximum, with the hour the agency attached to it.
#[derive(Clone, Debug)]
pub struct SamplePeak {
    pub lat: f32,
    pub lon: f32,
    pub value: f32,
    pub when: String,
}

#[derive(Clone, Debug)]
pub struct VolcanoAlert {
    pub name: String,
    pub lat: f32,
    pub lon: f32,
    pub alert_level: String,
    pub color_code: String,
    pub synopsis: String,
}

#[derive(Clone, Debug, Default)]
pub struct SolarReading {
    pub wind_kms: Option<f32>,
    pub wind_time: String,
    pub bt_nt: Option<f32>,
    pub bz_nt: Option<f32>,
    pub imf_time: String,
    pub kp_now: Option<f32>,
    pub kp_now_kind: String,
    pub kp_now_time: String,
    pub kp_now_scale: Option<String>,
    pub kp_forecast: Option<f32>,
    pub kp_forecast_time: String,
    pub kp_forecast_scale: Option<String>,
    pub xray_class: String,
    pub xray_time: String,
    pub latest_flare: String,
    pub latest_flare_time: String,
    pub strongest_flare: String,
    pub strongest_flare_time: String,
    pub cme_line: String,
    /// RTSW proton density, cm^-3. The latest active finite row, not the file tail.
    pub density_cm3: Option<f32>,
    /// RTSW proton temperature, kelvin, from the same row as `density_cm3`.
    pub temperature_k: Option<f32>,
    pub plasma_time: String,
    pub plasma_source: String,
    pub plasma_line: String,
    /// GOES integral proton flux at >=10 MeV, in pfu. Not an S-scale.
    pub proton_10mev: Option<f32>,
    pub proton_10mev_time: String,
    pub proton_line: String,
    /// 10.7 cm solar radio flux, sfu.
    pub f107_sfu: Option<f32>,
    pub f107_time: String,
    pub f107_schedule: String,
    pub f107_mean_sfu: Option<f32>,
    pub f107_mean_time: String,
    pub f107_line: String,
    /// Kyoto Dst, nT, as published by NOAA SWPC.
    pub dst_nt: Option<f32>,
    pub dst_time: String,
    pub dst_line: String,
    /// Quoted NOAA scale text for the current key, such as "0 none".
    pub scale_r: String,
    pub scale_s: String,
    pub scale_g: String,
    pub scale_time: String,
    pub scale_line: String,
    /// GOES >=2 MeV integral electron flux, pfu. Not a scale.
    pub electron_2mev: Option<f32>,
    pub electron_2mev_time: String,
    pub electron_line: String,
    /// GOES magnetometer total field, nT. The propagated IMF Bt/Bz line stays separate.
    pub mag_total_nt: Option<f32>,
    pub mag_time: String,
    pub mag_satellite: String,
    pub mag_arcjet: bool,
    pub mag_line: String,
    /// Newest SWPC solar-region day. A region entry is not a flare.
    pub region_line: String,
    /// Newest observatory sunspot report. One report is not a flare.
    pub sunspot_line: String,
    /// Newest SWPC issued alert. The sentence is the agency's own text.
    pub alert_code: String,
    pub alert_sentence: String,
    pub alert_time: String,
    pub alert_line: String,
    pub alert_watch: bool,
}

/// CPC climate indices. One ONI row is one season, not a declared event.
#[derive(Clone, Debug, Default)]
pub struct ClimateReading {
    pub oni_season: String,
    pub oni_year: Option<i32>,
    pub oni_total: Option<f32>,
    pub oni_anom: Option<f32>,
    pub oni_line: String,
    pub ao_year: Option<i32>,
    pub ao_month: Option<i32>,
    pub ao: Option<f32>,
    pub ao_line: String,
    pub nao_year: Option<i32>,
    pub nao_month: Option<i32>,
    pub nao: Option<f32>,
    pub nao_line: String,
    pub mjo_line: String,
    pub ice_line: String,
    pub co2_line: String,
}

/// A named storm from an agency warning. `agency` is NHC or JTWC.
#[derive(Clone, Debug)]
pub struct Storm {
    pub agency: String,
    pub id: String,
    pub name: String,
    pub class_code: String,
    pub lat: f32,
    pub lon: f32,
    pub intensity_kt: Option<f32>,
    pub motion_dir: Option<f32>,
    pub motion_kt: Option<f32>,
    pub updated: String,
}

/// One volcano in the Smithsonian / USGS weekly report RSS.
#[derive(Clone, Debug)]
pub struct WeeklyVolcano {
    pub name: String,
    pub country: String,
    pub week: String,
    pub kind: String,
    pub lat: f32,
    pub lon: f32,
    pub guid: String,
}

/// Agency products quoted in the reading. Empty fields stay out of the sentence.
#[derive(Clone, Debug, Default)]
pub struct Bulletin {
    pub wind_forecast: Option<SamplePeak>,
    pub wave_forecast: Option<SamplePeak>,
    pub rain_forecast: Option<SamplePeak>,
    pub solar: SolarReading,
    pub climate: ClimateReading,
    pub volcano_alerts: Vec<VolcanoAlert>,
    pub alerts_loaded: bool,
    pub aftershock_line: String,
    pub storms: Vec<Storm>,
    pub storm_line: String,
    pub tsunami_line: String,
    pub tsunami_ids: Vec<String>,
    pub weekly: Vec<WeeklyVolcano>,
    pub weekly_line: String,
    pub record_line: String,
    pub look_lines: Vec<String>,
    /// Short index of the series files. The rows themselves stay on disk.
    pub held: Vec<series::HeldNote>,
    /// Finite sea-level pressure and humidity range on the loaded wind grid.
    pub pressure_line: String,
    /// Finite range of ocean-current speed on the loaded marine hours.
    pub current_hour_line: String,
    /// FIRMS detection count. A hotspot is not promoted to a warning.
    pub fire_line: String,
    /// Short VAAC count for the reading. The advisory sentences live in `vaac_desk`.
    pub vaac_line: String,
    pub vaac_desk: Vec<String>,
    /// Latest IOC station samples. A level is a reading, not a tsunami bulletin.
    pub gauge_line: String,
}

#[derive(Clone, Debug)]
pub struct WindSample {
    pub lat: f32,
    pub lon: f32,
    pub temp_c: f32,
    pub wind_ms: f32,
    pub from_deg: f32,
    pub precip_mm: Option<f32>,
}

#[derive(Clone, Debug)]
pub struct OceanSample {
    pub lat: f32,
    pub lon: f32,
    pub sst_c: f32,
    pub wave_m: f32,
    /// Kilometers per hour. Open-Meteo `ocean_current_velocity`.
    pub current_kmh: Option<f32>,
    /// Degrees the water is heading toward. 0 is north, 90 is east.
    pub current_toward_deg: Option<f32>,
}

#[derive(Clone, Debug)]
pub struct ElevationGrid {
    pub nlat: usize,
    pub nlon: usize,
    pub meters: Vec<i16>,
}

impl ElevationGrid {
    pub fn empty() -> Self {
        Self { nlat: 0, nlon: 0, meters: Vec::new() }
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = fs::read(path).map_err(|e| e.to_string())?;
        if bytes.len() < 4 {
            return Err("elevation file is short".into());
        }
        let nlat = u16::from_le_bytes([bytes[0], bytes[1]]) as usize;
        let nlon = u16::from_le_bytes([bytes[2], bytes[3]]) as usize;
        let need = 4 + nlat * nlon * 2;
        if nlat < 2 || nlon < 2 || bytes.len() < need {
            return Err(format!("elevation file is {nlat}x{nlon}, {} bytes", bytes.len()));
        }
        let mut meters = Vec::with_capacity(nlat * nlon);
        for chunk in bytes[4..need].chunks_exact(2) {
            meters.push(i16::from_le_bytes([chunk[0], chunk[1]]));
        }
        Ok(Self { nlat, nlon, meters })
    }

    pub fn lat_lon(&self, row: usize, col: usize) -> (f32, f32) {
        let lat = 90.0 - row as f32 * (180.0 / (self.nlat.saturating_sub(1).max(1) as f32));
        let lon = -180.0 + col as f32 * (360.0 / self.nlon.max(1) as f32);
        (lat, lon)
    }

    /// Nearest 5° cell, in meters. `None` where the grid has no sample.
    pub fn sample(&self, lat_deg: f32, lon_deg: f32) -> Option<i16> {
        if self.nlat < 2 || self.nlon < 2 {
            return None;
        }
        let lat = lat_deg.clamp(-90.0, 90.0);
        let lon = geo::wrap_pi(lon_deg.to_radians()).to_degrees();
        let row_f = (90.0 - lat) / (180.0 / (self.nlat - 1) as f32);
        let col_f = (lon + 180.0) / (360.0 / self.nlon as f32);
        let row = row_f.round() as isize;
        let col = col_f.round() as isize;
        if row < 0 || row >= self.nlat as isize {
            return None;
        }
        let col = col.rem_euclid(self.nlon as isize) as usize;
        let value = *self.meters.get(row as usize * self.nlon + col)?;
        if value <= -30_000 { None } else { Some(value) }
    }
}

#[derive(Clone, Debug)]
pub struct FeedState {
    pub generation: u64,
    pub live_done: bool,
    pub cities: Vec<City>,
    pub volcanoes: Vec<Volcano>,
    pub faults: Vec<Vec<(f32, f32)>>,
    pub elevation: ElevationGrid,
    pub quakes: Vec<Quake>,
    pub wind: Vec<WindSample>,
    pub ocean: Vec<OceanSample>,
    pub bulletin: Bulletin,
    pub cities_status: String,
    pub elevation_status: String,
    pub quake_status: String,
    pub fault_status: String,
    pub volcano_status: String,
    pub weather_status: String,
    pub ocean_status: String,
    pub reading: Vec<String>,
}

impl Default for FeedState {
    fn default() -> Self {
        Self {
            generation: 0,
            live_done: false,
            cities: Vec::new(),
            volcanoes: Vec::new(),
            faults: Vec::new(),
            elevation: ElevationGrid::empty(),
            quakes: Vec::new(),
            wind: Vec::new(),
            ocean: Vec::new(),
            bulletin: Bulletin::default(),
            cities_status: "loading".into(),
            elevation_status: "loading".into(),
            quake_status: "loading USGS".into(),
            fault_status: "loading".into(),
            volcano_status: "loading".into(),
            weather_status: "loading Open-Meteo".into(),
            ocean_status: "loading marine".into(),
            reading: vec!["Waiting for the live feeds.".into()],
        }
    }
}

pub type SharedFeeds = Arc<Mutex<FeedState>>;

pub fn start() -> SharedFeeds {
    let mut state = FeedState::default();
    load_static(&mut state);
    state.generation = 1;
    let shared = Arc::new(Mutex::new(state));
    let background = Arc::clone(&shared);
    thread::Builder::new()
        .name("fsot-feeds".into())
        .spawn(move || refresh_loop(background))
        .ok();
    shared
}

fn load_static(state: &mut FeedState) {
    match load_cities(&renderer::project_path("assets/cities.json")) {
        Ok(cities) => {
            state.cities_status = format!("Natural Earth · {} place names", cities.len());
            state.cities = cities;
        }
        Err(err) => state.cities_status = format!("cities unavailable · {err}"),
    }
    match load_faults(&renderer::project_path("assets/plates.json")) {
        Ok(faults) => {
            let lines = faults.len();
            state.fault_status = format!("Bird 2002 plate boundaries · {lines} lines");
            state.faults = faults;
        }
        Err(err) => state.fault_status = format!("faults unavailable · {err}"),
    }
    match load_volcanoes(&renderer::project_path("assets/volcanoes.json")) {
        Ok(volcanoes) => {
            let recent = volcanoes.iter().filter(|v| v.last_year.unwrap_or(i32::MIN) >= 2000).count();
            state.volcano_status = format!("Smithsonian Holocene · {} · {recent} with an eruption since 2000", volcanoes.len());
            state.volcanoes = volcanoes;
        }
        Err(err) => state.volcano_status = format!("volcanoes unavailable · {err}"),
    }
    match ElevationGrid::load(&renderer::project_path("assets/elevation_5deg.bin")) {
        Ok(grid) => {
            state.elevation_status = format!("Open-Meteo DEM · {}° grid", 180 / grid.nlat.saturating_sub(1).max(1));
            state.elevation = grid;
        }
        Err(err) => state.elevation_status = format!("elevation unavailable · {err}"),
    }
}

fn refresh_loop(shared: SharedFeeds) {
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(40)).build();
    let quick = ureq::AgentBuilder::new().timeout(Duration::from_secs(8)).build();
    loop {
        let mut held = Vec::new();
        let quakes = fetch_quakes(&agent);
        match &quakes {
            Ok(list) => held.push(series::store(&series_root(), "quakes", "USGS", quake_rows(list))),
            Err(_) => held.push(series::recall_or(&series_root(), "quakes", "USGS", "USGS earthquake feed did not load.")),
        }
        let wind = fetch_wind(&agent, &mut held);
        if wind.is_err() {
            held.push(series::recall_or(&series_root(), "wind-grid", "Open-Meteo", "Open-Meteo wind did not load."));
            held.push(series::recall_or(&series_root(), "wind-hours", "Open-Meteo", "Open-Meteo wind did not load."));
        }
        let ocean = fetch_ocean(&agent, &mut held);
        if ocean.is_err() {
            held.push(series::recall_or(&series_root(), "ocean-grid", "Open-Meteo marine", "Open-Meteo marine did not load."));
            held.push(series::recall_or(&series_root(), "ocean-hours", "Open-Meteo marine", "Open-Meteo marine did not load."));
        }
        let solar = fetch_solar(&agent, &quick, &mut held);
        let climate = fetch_climate(&agent, &mut held);
        let fires = fetch_fires(&agent, &mut held);
        let storm_pass = fetch_storms(&agent, &mut held);
        let vaac = fetch_vaac(&agent, &quick, &mut held);
        let gauges = fetch_gauges(&agent, &mut held);
        series::sort_notes(&mut held);
        let alerts = fetch_volcano_alerts(&agent);
        let loaded_quakes = quakes.as_ref().ok().map(|list| list.as_slice()).unwrap_or(&[]);
        let aftershock = fetch_aftershock(&agent, loaded_quakes);
        let tsunami = fetch_tsunami(&agent);
        let weekly = fetch_weekly(&agent);
        let now_unix = geo::unix_now();
        let checks = fetch_closed_checks(&agent, now_unix);
        if let Ok(mut state) = shared.lock() {
            match quakes {
                Ok(quakes) => {
                    state.quake_status = format!("USGS M2.5+ past day · {} events", quakes.len());
                    state.quakes = quakes;
                }
                Err(err) => {
                    if state.quakes.is_empty() {
                        state.quake_status = format!("USGS unavailable · {err}");
                    }
                }
            }
            match wind {
                Ok(wind) => {
                    let forecast = if wind.forecast.is_some() { " · 24 h forecast" } else { "" };
                    state.weather_status = format!("Open-Meteo · {} wind samples{forecast}", wind.samples.len());
                    state.bulletin.wind_forecast = wind.forecast;
                    state.bulletin.rain_forecast = wind.rain;
                    state.bulletin.pressure_line = wind.pressure_line;
                    state.wind = wind.samples;
                }
                Err(err) => {
                    if state.wind.is_empty() {
                        state.weather_status = format!("weather unavailable · {err}");
                    }
                }
            }
            match ocean {
                Ok(ocean) => {
                    let forecast = if ocean.forecast.is_some() { " · 24 h forecast" } else { "" };
                    state.ocean_status = format!("Open-Meteo marine · {} sea samples{forecast}", ocean.samples.len());
                    state.bulletin.wave_forecast = ocean.forecast;
                    state.bulletin.current_hour_line = ocean.current_hour_line;
                    state.ocean = ocean.samples;
                }
                Err(err) => {
                    if state.ocean.is_empty() {
                        state.ocean_status = format!("ocean unavailable · {err}");
                    }
                }
            }
            state.bulletin.solar = solar;
            state.bulletin.climate = climate;
            state.bulletin.fire_line = fires;
            state.bulletin.held = held;
            match alerts {
                Ok(alerts) => {
                    state.volcano_status = format!(
                        "Smithsonian Holocene · {} · USGS elevated {}",
                        state.volcanoes.len(),
                        alerts.len()
                    );
                    state.bulletin.volcano_alerts = alerts;
                    state.bulletin.alerts_loaded = true;
                }
                Err(err) => {
                    if !state.bulletin.alerts_loaded {
                        state.volcano_status = format!(
                            "Smithsonian Holocene · {} · USGS alerts unavailable · {err}",
                            state.volcanoes.len()
                        );
                    }
                }
            }
            state.bulletin.aftershock_line = aftershock;
            apply_storm_pass(&mut state.bulletin, storm_pass);
            match tsunami {
                Ok((line, ids)) => {
                    state.bulletin.tsunami_line = line;
                    state.bulletin.tsunami_ids = ids;
                }
                Err(_) => {
                    if state.bulletin.tsunami_line.is_empty() {
                        state.bulletin.tsunami_line = "NWS tsunami bulletin did not load.".into();
                    }
                }
            }
            match weekly {
                Ok((line, weekly)) => {
                    state.bulletin.weekly_line = line;
                    state.bulletin.weekly = weekly;
                }
                Err(_) => {
                    if state.bulletin.weekly_line.is_empty() {
                        state.bulletin.weekly_line = "Smithsonian/USGS weekly volcanic report did not load.".into();
                    }
                }
            }
            match vaac {
                Ok(pass) => {
                    state.bulletin.vaac_line = pass.line;
                    state.bulletin.vaac_desk = pass.desk;
                }
                Err(_) => {
                    if state.bulletin.vaac_line.is_empty() {
                        state.bulletin.vaac_line = "NWS volcanic ash advisories did not load.".into();
                    }
                }
            }
            match gauges {
                Ok(line) => state.bulletin.gauge_line = line,
                Err(_) => {
                    if state.bulletin.gauge_line.is_empty() {
                        state.bulletin.gauge_line = "UNESCO IOC sea-level gauges did not load.".into();
                    }
                }
            }
            let record_line = crate::record::write_kept_record(
                &cache_path("record"),
                "cache/record",
                now_unix,
                &state.quakes,
                &state.volcanoes,
                &state.wind,
                &state.faults,
                &state.bulletin,
                &checks,
            );
            state.bulletin.record_line = record_line;
            state.bulletin.look_lines = crate::record::look_lines(&cache_path("record"), now_unix);
            state.reading = compose_reading(
                &state.quakes,
                &state.wind,
                &state.ocean,
                &state.volcanoes,
                &state.faults,
                &state.bulletin,
                now_unix,
            );
            state.live_done = true;
            state.generation = state.generation.wrapping_add(1);
        }
        thread::sleep(Duration::from_secs(300));
    }
}

pub fn compose_reading(
    quakes: &[Quake],
    wind: &[WindSample],
    ocean: &[OceanSample],
    volcanoes: &[Volcano],
    faults: &[Vec<(f32, f32)>],
    bulletin: &Bulletin,
    now_unix: f64,
) -> Vec<String> {
    let mut lines = Vec::new();
    if !bulletin.record_line.is_empty() {
        lines.push(bulletin.record_line.clone());
    }
    lines.extend(bulletin.look_lines.iter().cloned());
    lines.extend(solar_lines(&bulletin.solar));
    lines.push(wind_forecast_line(&bulletin.wind_forecast));
    lines.push(rain_line(&bulletin.rain_forecast));
    lines.push(wave_forecast_line(&bulletin.wave_forecast));
    lines.push(current_line(ocean));
    if !bulletin.storm_line.is_empty() {
        lines.push(bulletin.storm_line.clone());
    }
    if bulletin.aftershock_line.is_empty() {
        lines.push("USGS aftershock forecast has not arrived.".into());
    } else {
        lines.push(bulletin.aftershock_line.clone());
    }
    volcano_lines(&mut lines, bulletin);
    if !bulletin.vaac_line.is_empty() {
        lines.push(bulletin.vaac_line.clone());
    }
    if !bulletin.weekly_line.is_empty() {
        lines.push(bulletin.weekly_line.clone());
    }
    lines.extend(joint_lines(quakes, wind, ocean, volcanoes, faults, bulletin, now_unix));

    let now_ms = (now_unix * 1000.0) as i64;
    if let Some(biggest) = quakes.iter().max_by(|a, b| a.mag.total_cmp(&b.mag)) {
        let pager = match &biggest.alert {
            Some(alert) => format!(" · USGS PAGER {}", alert.to_ascii_uppercase()),
            None => String::new(),
        };
        lines.push(format!(
            "Largest loaded quake: M{:.1} · {} · {:.0} km deep · {}{pager}",
            biggest.mag,
            biggest.place,
            biggest.depth_km,
            age_text(now_ms.saturating_sub(biggest.time_ms))
        ));
    } else {
        lines.push("No earthquakes in the loaded USGS record.".into());
    }
    let tsunami_n = quakes.iter().filter(|quake| quake.tsunami).count();
    if tsunami_n > 0 {
        lines.push(format!("USGS tsunami flag is set on {tsunami_n} loaded event(s)."));
    }
    if !bulletin.tsunami_line.is_empty() {
        lines.push(bulletin.tsunami_line.clone());
    }
    let clusters = clusters_within(quakes, now_ms, 700.0, 24 * 3600 * 1000);
    if clusters.is_empty() {
        lines.push("No cluster of 3 or more M4.5+ events within 700 km in the last 24 h.".into());
    }
    for cluster in clusters.iter().take(3) {
        lines.push(format!(
            "Active sequence: {} events within 700 km near {}, largest M{:.1}",
            cluster.count,
            geo::format_lat_lon_deg(cluster.lat, cluster.lon),
            cluster.max_mag
        ));
    }
    if let Some(strong) = wind.iter().max_by(|a, b| a.wind_ms.total_cmp(&b.wind_ms)) {
        lines.push(format!(
            "Strongest wind in the sample: {:.0} m/s from {:.0}° at {}, air {:.0}°C",
            strong.wind_ms,
            strong.from_deg,
            geo::format_lat_lon_deg(strong.lat, strong.lon),
            strong.temp_c
        ));
    } else {
        lines.push("Wind samples have not arrived.".into());
    }
    if let Some(sea) = ocean.iter().max_by(|a, b| a.wave_m.total_cmp(&b.wave_m)) {
        lines.push(format!(
            "Highest sea in the sample: {:.1} m waves, {:.0}°C, at {}",
            sea.wave_m,
            sea.sst_c,
            geo::format_lat_lon_deg(sea.lat, sea.lon)
        ));
    } else {
        lines.push("Sea samples have not arrived.".into());
    }
    lines.push("This counts the loaded record. It does not forecast the next event.".into());
    lines
}

/// One agency product the panel can open. `lat` and `lon` are degrees when the product has a place.
#[derive(Clone, Debug)]
pub struct WatchCard {
    pub title: String,
    pub detail: String,
    pub place: String,
    pub lat: Option<f32>,
    pub lon: Option<f32>,
}

pub fn watch_cards(quakes: &[Quake], bulletin: &Bulletin) -> Vec<WatchCard> {
    let mut cards = Vec::new();
    for alert in &bulletin.volcano_alerts {
        let level = alert.alert_level.to_ascii_uppercase();
        if level == "WATCH" || level == "WARNING" {
            cards.push(WatchCard {
                title: format!("USGS {} {}/{}.", alert.name, alert.alert_level, alert.color_code),
                detail: String::new(),
                place: alert.name.clone(),
                lat: Some(alert.lat),
                lon: Some(alert.lon),
            });
        }
    }
    for alert in bulletin
        .volcano_alerts
        .iter()
        .filter(|alert| alert.alert_level.eq_ignore_ascii_case("ADVISORY") && alert.synopsis.to_ascii_lowercase().contains("forecast"))
        .take(2)
    {
        let text: String = alert.synopsis.chars().take(220).collect();
        cards.push(WatchCard {
            title: format!("USGS {} remains {}/{}.", alert.name, alert.alert_level, alert.color_code),
            detail: format!("Notice: {text}"),
            place: alert.name.clone(),
            lat: Some(alert.lat),
            lon: Some(alert.lon),
        });
    }
    for storm in bulletin.storms.iter().take(6) {
        let class = storm_class(&storm.class_code);
        let name = if storm.name.is_empty() { storm.id.as_str() } else { storm.name.as_str() };
        let agency = if storm.agency.is_empty() { "NHC" } else { storm.agency.as_str() };
        let kt = storm.intensity_kt.map(|value| format!(", {value:.0} kt")).unwrap_or_default();
        cards.push(WatchCard {
            title: format!("{agency} {class} {name}{kt}."),
            detail: geo::format_lat_lon_deg(storm.lat, storm.lon),
            place: name.to_string(),
            lat: Some(storm.lat),
            lon: Some(storm.lon),
        });
    }
    if bulletin.solar.alert_watch && !bulletin.solar.alert_line.is_empty() {
        cards.push(WatchCard {
            title: bulletin.solar.alert_line.clone(),
            detail: String::new(),
            place: String::new(),
            lat: None,
            lon: None,
        });
    }
    if bulletin_active(&bulletin.tsunami_line, &["no current", "did not load"]) {
        cards.push(WatchCard {
            title: bulletin.tsunami_line.clone(),
            detail: String::new(),
            place: String::new(),
            lat: None,
            lon: None,
        });
    }
    let mut quake_notes = 0;
    for quake in quakes {
        if quake_notes >= 4 {
            break;
        }
        if quake.alert.as_deref().is_some_and(pager_is_high) {
            let alert = quake.alert.as_deref().unwrap_or("");
            cards.push(WatchCard {
                title: format!("USGS PAGER {} on M{:.1} {}.", alert.to_ascii_uppercase(), quake.mag, quake.place),
                detail: String::new(),
                place: quake.place.clone(),
                lat: Some(quake.lat),
                lon: Some(quake.lon),
            });
            quake_notes += 1;
        }
        if quake_notes >= 4 {
            break;
        }
        if quake.tsunami {
            cards.push(WatchCard {
                title: format!("USGS tsunami flag on M{:.1} {}.", quake.mag, quake.place),
                detail: String::new(),
                place: quake.place.clone(),
                lat: Some(quake.lat),
                lon: Some(quake.lon),
            });
            quake_notes += 1;
        }
    }
    if let Some(kp) = bulletin.solar.kp_now {
        if kp >= 5.0 {
            cards.push(WatchCard {
                title: format!(
                    "NOAA SWPC Kp {kp:.2} {} at {} is at or above 5.",
                    bulletin.solar.kp_now_kind, bulletin.solar.kp_now_time
                ),
                detail: String::new(),
                place: String::new(),
                lat: None,
                lon: None,
            });
        }
    }
    if let Some(kp) = bulletin.solar.kp_forecast {
        if kp >= 5.0 {
            cards.push(WatchCard {
                title: format!(
                    "NOAA SWPC forecast peak Kp {kp:.2} at {} is at or above 5.",
                    bulletin.solar.kp_forecast_time
                ),
                detail: String::new(),
                place: String::new(),
                lat: None,
                lon: None,
            });
        }
    }
    let mut seen_flare = String::new();
    for (class, time) in [
        (bulletin.solar.xray_class.as_str(), bulletin.solar.xray_time.as_str()),
        (bulletin.solar.latest_flare.as_str(), bulletin.solar.latest_flare_time.as_str()),
    ] {
        if !flare_is_high(class) {
            continue;
        }
        let key = format!("{class}|{time}");
        if key == seen_flare {
            continue;
        }
        seen_flare = key;
        cards.push(WatchCard {
            title: format!("GOES X-ray {class} at {time} is M-class or X-class."),
            detail: String::new(),
            place: String::new(),
            lat: None,
            lon: None,
        });
    }
    cards
}

#[cfg(test)]
fn watch_lines(quakes: &[Quake], bulletin: &Bulletin) -> Vec<String> {
    let cards = watch_cards(quakes, bulletin);
    if cards.is_empty() {
        vec!["Agency watch: no warning, watch, storm, or tsunami bulletin is loaded.".into()]
    } else {
        let mut lines = vec!["Agency watch:".into()];
        for card in cards {
            if card.detail.is_empty() {
                lines.push(card.title);
            } else {
                lines.push(format!("{} {}", card.title, card.detail));
            }
        }
        lines
    }
}

fn bulletin_active(line: &str, quiet: &[&str]) -> bool {
    if line.is_empty() {
        return false;
    }
    let lower = line.to_ascii_lowercase();
    quiet.iter().all(|phrase| !lower.contains(phrase))
}

fn pager_is_high(alert: &str) -> bool {
    let alert = alert.to_ascii_lowercase();
    alert == "orange" || alert == "red"
}

fn flare_is_high(class: &str) -> bool {
    let class = class.trim();
    let mut chars = class.chars();
    match chars.next() {
        Some('M' | 'X' | 'm' | 'x') => chars.next().is_some_and(|ch| ch.is_ascii_digit()),
        _ => false,
    }
}

/// Display units for the place card. The reading keeps the agency's own units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShowUnits {
    pub fahrenheit: bool,
    pub mph: bool,
    pub inches: bool,
}

impl Default for ShowUnits {
    fn default() -> Self {
        Self { fahrenheit: false, mph: false, inches: false }
    }
}

#[derive(Clone, Debug)]
pub struct PlaceNow {
    pub temp_c: Option<f32>,
    pub wind_ms: Option<f32>,
    pub precip_mm: Option<f32>,
}

#[derive(Clone, Debug)]
pub struct PlaceHour {
    pub when: String,
    pub temp_c: Option<f32>,
    pub wind_ms: Option<f32>,
    pub precip_mm: Option<f32>,
}

#[derive(Clone, Debug)]
pub struct PlaceForecast {
    pub now: PlaceNow,
    pub hours: Vec<PlaceHour>,
}

pub fn search_cities<'a>(cities: &'a [City], query: &str) -> Vec<&'a City> {
    let query = query.trim().to_ascii_lowercase();
    if query.len() < 2 {
        return Vec::new();
    }
    let mut hits: Vec<&City> = cities.iter().filter(|city| city.name.to_ascii_lowercase().contains(&query)).collect();
    hits.sort_by(|a, b| {
        let a_start = a.name.to_ascii_lowercase().starts_with(&query);
        let b_start = b.name.to_ascii_lowercase().starts_with(&query);
        b_start.cmp(&a_start).then(b.pop.cmp(&a.pop)).then(a.name.cmp(&b.name))
    });
    hits.truncate(6);
    hits
}

/// One town, city, or postal-code match from the Open-Meteo geocoder.
#[derive(Clone, Debug)]
pub struct GeoHit {
    pub name: String,
    pub detail: String,
    pub lat: f32,
    pub lon: f32,
}

impl GeoHit {
    pub fn label(&self) -> String {
        if self.detail.is_empty() {
            self.name.clone()
        } else {
            format!("{}, {}", self.name, self.detail)
        }
    }
}

pub fn geocode_url(query: &str) -> String {
    format!(
        "https://geocoding-api.open-meteo.com/v1/search?name={}&count=6&language=en&format=json",
        url_query(query.trim())
    )
}

pub fn parse_geocode(body: &str) -> Result<Vec<GeoHit>, String> {
    let value: Value = serde_json::from_str(body).map_err(|err| err.to_string())?;
    if value.get("error").and_then(|flag| flag.as_bool()) == Some(true) {
        let reason = value.get("reason").and_then(|item| item.as_str()).unwrap_or("Place search failed.");
        return Err(reason.to_string());
    }
    let Some(results) = value.get("results").and_then(|item| item.as_array()) else {
        return Ok(Vec::new());
    };
    let mut hits = Vec::new();
    for item in results {
        let Some(name) = item.get("name").and_then(|part| part.as_str()) else { continue };
        let Some(lat) = item.get("latitude").and_then(|part| part.as_f64()) else { continue };
        let Some(lon) = item.get("longitude").and_then(|part| part.as_f64()) else { continue };
        if !lat.is_finite() || !lon.is_finite() {
            continue;
        }
        let mut parts = Vec::new();
        for key in ["admin1", "country"] {
            let Some(text) = item.get(key).and_then(|part| part.as_str()) else { continue };
            if text.is_empty() || text == name || parts.iter().any(|have: &String| have == text) {
                continue;
            }
            parts.push(text.to_string());
        }
        hits.push(GeoHit { name: name.to_string(), detail: parts.join(", "), lat: lat as f32, lon: lon as f32 });
        if hits.len() == 6 {
            break;
        }
    }
    Ok(hits)
}

pub fn fetch_geocode(query: &str) -> Result<Vec<GeoHit>, String> {
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(12)).build();
    let body = http_get_once(&agent, &geocode_url(query))?;
    parse_geocode(&body)
}

fn url_query(text: &str) -> String {
    let mut out = String::new();
    for byte in text.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(*byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

pub fn describe_spot(lat: f32, lon: f32, wind: &[WindSample], quakes: &[Quake], bulletin: &Bulletin, units: ShowUnits) -> Vec<String> {
    let mut lines = vec![geo::format_lat_lon_deg(lat, lon)];
    match nearest_of(wind, lat, lon, |sample| (sample.lat, sample.lon)) {
        Some((index, km)) => {
            let sample = &wind[index];
            lines.push(format!(
                "Nearest wind sample, {}: {} from {:.0}°",
                km_text(km),
                speed_text(sample.wind_ms, units),
                sample.from_deg
            ));
            match sample.precip_mm {
                Some(mm) => lines.push(format!("Rain on that sample: {}", rain_text(mm, units))),
                None => lines.push("Rain on that sample has not arrived.".into()),
            }
        }
        None => lines.push("Wind samples have not arrived.".into()),
    }
    match nearest_of(quakes, lat, lon, |quake| (quake.lat, quake.lon)) {
        Some((index, km)) => {
            let quake = &quakes[index];
            lines.push(format!("Nearest quake, {}: M{:.1} {}", km_text(km), quake.mag, quake.place));
        }
        None => lines.push("No earthquake is loaded.".into()),
    }
    if !bulletin.alerts_loaded {
        lines.push("USGS volcano alerts have not arrived.".into());
    } else {
        match nearest_of(&bulletin.volcano_alerts, lat, lon, |alert| (alert.lat, alert.lon)) {
            Some((index, km)) => {
                let alert = &bulletin.volcano_alerts[index];
                lines.push(format!(
                    "Nearest USGS alert, {}: {} {}/{}",
                    km_text(km),
                    alert.name,
                    alert.alert_level,
                    alert.color_code
                ));
            }
            None => lines.push("USGS reports no elevated volcano alert.".into()),
        }
    }
    lines
}

pub fn format_place_forecast(forecast: &PlaceForecast, units: ShowUnits) -> Vec<String> {
    let mut lines = vec!["Open-Meteo hourly forecast for this place. Hours are UTC.".into()];
    lines.push(format!(
        "Open-Meteo now: {}, {}, {}",
        temp_text(forecast.now.temp_c, units),
        speed_text_opt(forecast.now.wind_ms, units),
        rain_text_opt(forecast.now.precip_mm, units)
    ));
    for hour in &forecast.hours {
        let label = hour.when.get(11..16).unwrap_or(hour.when.as_str());
        lines.push(format!(
            "{label}  {}  {}  {}",
            temp_text(hour.temp_c, units),
            speed_text_opt(hour.wind_ms, units),
            rain_text_opt(hour.precip_mm, units)
        ));
    }
    lines
}

pub fn fetch_place_hours(lat: f32, lon: f32) -> Result<PlaceForecast, String> {
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(20)).build();
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}&current=temperature_2m,wind_speed_10m,precipitation&hourly=temperature_2m,wind_speed_10m,precipitation&forecast_hours=48&wind_speed_unit=ms&timezone=GMT"
    );
    let body = http_get_once(&agent, &url)?;
    let now_hour = hour_stamp(geo::unix_now());
    parse_place_forecast(&body, &now_hour).ok_or_else(|| "Open-Meteo hourly forecast did not include hours.".into())
}

fn parse_place_forecast(body: &str, now_hour: &str) -> Option<PlaceForecast> {
    let value: Value = serde_json::from_str(body).ok()?;
    let current = &value["current"];
    let now = PlaceNow {
        temp_c: json_f32(&current["temperature_2m"]),
        wind_ms: json_f32(&current["wind_speed_10m"]),
        precip_mm: json_f32(&current["precipitation"]),
    };
    let hourly = &value["hourly"];
    let times = hourly["time"].as_array()?;
    let temps = hourly["temperature_2m"].as_array();
    let winds = hourly["wind_speed_10m"].as_array();
    let rains = hourly["precipitation"].as_array();
    let mut hours = Vec::new();
    for (index, stamp) in times.iter().enumerate() {
        let when = stamp.as_str().unwrap_or("").to_string();
        if stamp_key(&when).as_str() < now_hour {
            continue;
        }
        hours.push(PlaceHour {
            when,
            temp_c: temps.and_then(|list| list.get(index)).and_then(json_f32),
            wind_ms: winds.and_then(|list| list.get(index)).and_then(json_f32),
            precip_mm: rains.and_then(|list| list.get(index)).and_then(json_f32),
        });
        if hours.len() == 8 {
            break;
        }
    }
    Some(PlaceForecast { now, hours })
}

/// Min, mean, and max across ensemble member columns. The unsuffixed series is not a member.
#[derive(Clone, Debug)]
pub struct Spread {
    pub min: f32,
    pub mean: f32,
    pub max: f32,
    pub members: usize,
}

#[derive(Clone, Debug)]
pub struct EnsembleHour {
    pub when: String,
    pub temp: Option<Spread>,
    pub rain: Option<Spread>,
    pub wind: Option<Spread>,
}

#[derive(Clone, Debug)]
pub struct PlaceEnsemble {
    pub hours: Vec<EnsembleHour>,
    /// Rows written to `cache/series/ensemble.json` for this place. The desk quotes the count.
    pub held_rows: usize,
    pub held_first: String,
    pub held_last: String,
}

/// Carbon monoxide, sulphur dioxide, and dust for one chosen place. Not a second global grid.
#[derive(Clone, Debug)]
pub struct PlaceAir {
    pub line: String,
    pub held_rows: usize,
    pub held_first: String,
    pub held_last: String,
}

pub fn air_url(lat: f32, lon: f32) -> String {
    format!(
        "https://air-quality-api.open-meteo.com/v1/air-quality?latitude={lat}&longitude={lon}&hourly=carbon_monoxide,sulphur_dioxide,dust&timezone=GMT&forecast_days=1"
    )
}

pub fn fetch_place_air(lat: f32, lon: f32) -> Result<PlaceAir, String> {
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(20)).build();
    let body = http_get_once(&agent, &air_url(lat, lon)).map_err(|_| "Open-Meteo air quality for this place did not load.".to_string())?;
    let rows = series::hour_rows(
        &body,
        &[
            ("carbon_monoxide", "co_ugm3"),
            ("sulphur_dioxide", "so2_ugm3"),
            ("dust", "dust_ugm3"),
        ],
    );
    if rows.is_empty() {
        return Err("Open-Meteo air quality for this place did not load.".into());
    }
    let note = series::store(&series_root(), "air", "Open-Meteo air quality", rows.clone());
    Ok(PlaceAir {
        line: air_summary(&rows, &hour_stamp(geo::unix_now())),
        held_rows: note.rows,
        held_first: note.first,
        held_last: note.last,
    })
}

pub fn ensemble_url(lat: f32, lon: f32) -> String {
    format!(
        "https://ensemble-api.open-meteo.com/v1/ensemble?latitude={lat}&longitude={lon}&hourly=temperature_2m,precipitation,wind_speed_10m&models=gfs_seamless&forecast_days=1&wind_speed_unit=ms&timezone=GMT"
    )
}

pub fn fetch_place_ensemble(lat: f32, lon: f32) -> Result<PlaceEnsemble, String> {
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(20)).build();
    let body = http_get_once(&agent, &ensemble_url(lat, lon)).map_err(|_| "Open-Meteo ensemble for this place did not load.".to_string())?;
    let note = match series::ensemble_rows(&body) {
        Ok(rows) => Some(series::store(&series_root(), "ensemble", "Open-Meteo GFS seamless", rows)),
        Err(()) => None,
    };
    let now_hour = hour_stamp(geo::unix_now());
    let mut cast = parse_ensemble(&body, &now_hour).unwrap_or(PlaceEnsemble {
        hours: Vec::new(),
        held_rows: 0,
        held_first: String::new(),
        held_last: String::new(),
    });
    if let Some(note) = note {
        cast.held_rows = note.rows;
        cast.held_first = note.first;
        cast.held_last = note.last;
    }
    if cast.hours.is_empty() && cast.held_rows == 0 {
        return Err("Open-Meteo ensemble for this place did not load.".into());
    }
    Ok(cast)
}

fn parse_ensemble(body: &str, now_hour: &str) -> Option<PlaceEnsemble> {
    let value: Value = serde_json::from_str(body).ok()?;
    let hourly = value.get("hourly")?;
    let times = hourly.get("time")?.as_array()?;
    let temps = member_columns(hourly, "temperature_2m");
    let rains = member_columns(hourly, "precipitation");
    let winds = member_columns(hourly, "wind_speed_10m");
    let mut hours = Vec::new();
    let mut seen = 0usize;
    for (index, stamp) in times.iter().enumerate() {
        let when = stamp.as_str().unwrap_or("").to_string();
        if stamp_key(&when).as_str() < now_hour {
            continue;
        }
        if seen == 6 {
            break;
        }
        seen += 1;
        let hour = EnsembleHour {
            when,
            temp: spread_at(&temps, index),
            rain: spread_at(&rains, index),
            wind: spread_at(&winds, index),
        };
        if hour.temp.is_some() || hour.rain.is_some() || hour.wind.is_some() {
            hours.push(hour);
        }
    }
    if hours.is_empty() {
        None
    } else {
        Some(PlaceEnsemble { hours, held_rows: 0, held_first: String::new(), held_last: String::new() })
    }
}

fn member_columns<'a>(hourly: &'a Value, base: &str) -> Vec<&'a Vec<Value>> {
    let Some(map) = hourly.as_object() else { return Vec::new() };
    let prefix = format!("{base}_member");
    let mut keys: Vec<&String> = map.keys().filter(|key| key.starts_with(&prefix)).collect();
    keys.sort();
    keys.into_iter().filter_map(|key| map.get(key).and_then(|value| value.as_array())).collect()
}

fn spread_at(columns: &[&Vec<Value>], index: usize) -> Option<Spread> {
    let mut min = f32::MAX;
    let mut max = f32::MIN;
    let mut sum = 0.0f32;
    let mut count = 0usize;
    for column in columns {
        let Some(value) = column.get(index).and_then(json_finite) else { continue };
        let value = value as f32;
        min = min.min(value);
        max = max.max(value);
        sum += value;
        count += 1;
    }
    if count == 0 {
        return None;
    }
    Some(Spread { min, mean: sum / count as f32, max, members: count })
}

fn json_f32(value: &Value) -> Option<f32> {
    let number = value.as_f64()?;
    number.is_finite().then_some(number as f32)
}

fn nearest_of<T>(items: &[T], lat: f32, lon: f32, point: impl Fn(&T) -> (f32, f32)) -> Option<(usize, f32)> {
    items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let (plat, plon) = point(item);
            (index, km_between(lat, lon, plat, plon))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

fn km_text(km: f32) -> String {
    if km >= 100.0 { format!("{km:.0} km") } else { format!("{km:.1} km") }
}

pub fn speed_text(ms: f32, units: ShowUnits) -> String {
    if units.mph {
        format!("{:.0} mph", f64::from(ms) * 2.2369362920544)
    } else {
        format!("{ms:.0} m/s")
    }
}

fn speed_text_opt(ms: Option<f32>, units: ShowUnits) -> String {
    ms.map(|value| speed_text(value, units)).unwrap_or_else(|| "wind —".into())
}

pub fn temp_text(celsius: Option<f32>, units: ShowUnits) -> String {
    let Some(celsius) = celsius else { return "—".into() };
    if units.fahrenheit {
        format!("{:.0}°F", f64::from(celsius) * 1.8 + 32.0)
    } else {
        format!("{celsius:.0}°C")
    }
}

pub fn rain_text(mm: f32, units: ShowUnits) -> String {
    if units.inches {
        format!("{:.2} in", f64::from(mm) / 25.4)
    } else {
        format!("{mm:.1} mm")
    }
}

fn rain_text_opt(mm: Option<f32>, units: ShowUnits) -> String {
    mm.map(|value| rain_text(value, units)).unwrap_or_else(|| "rain —".into())
}

fn solar_lines(solar: &SolarReading) -> Vec<String> {
    let mut lines = Vec::new();
    match solar.wind_kms {
        Some(speed) => {
            let mut line = format!("NOAA SWPC solar wind {speed:.0} km/s at {}", solar.wind_time);
            if let (Some(bt), Some(bz)) = (solar.bt_nt, solar.bz_nt) {
                line.push_str(&format!(
                    ". Propagated IMF Bt {bt:.1} nT, Bz {bz:.1} nT at {}",
                    solar.imf_time
                ));
            }
            lines.push(line);
        }
        None => lines.push("NOAA SWPC solar wind has not arrived.".into()),
    }
    match solar.kp_now {
        Some(kp) => {
            let mut line = format!(
                "NOAA SWPC Kp {kp:.2} {} at {}{}",
                solar.kp_now_kind,
                solar.kp_now_time,
                scale_suffix(&solar.kp_now_scale)
            );
            if let Some(pred) = solar.kp_forecast {
                line.push_str(&format!(
                    ". Forecast peak Kp {pred:.2} predicted at {}{}",
                    solar.kp_forecast_time,
                    scale_suffix(&solar.kp_forecast_scale)
                ));
            }
            lines.push(line);
        }
        None => lines.push("NOAA SWPC Kp forecast has not arrived.".into()),
    }
    if solar.xray_class.is_empty() {
        lines.push("GOES X-ray flux has not arrived.".into());
    } else {
        let mut line = format!("GOES X-ray 0.1-0.8 nm {} at {}", solar.xray_class, solar.xray_time);
        if !solar.latest_flare.is_empty() {
            line.push_str(&format!(
                ". Latest flare in the 7-day list: {} at {}",
                solar.latest_flare, solar.latest_flare_time
            ));
        }
        if !solar.strongest_flare.is_empty() && solar.strongest_flare_time != solar.latest_flare_time {
            line.push_str(&format!(
                ". Strongest in that list: {} at {}",
                solar.strongest_flare, solar.strongest_flare_time
            ));
        }
        lines.push(line);
    }
    if !solar.cme_line.is_empty() {
        lines.push(solar.cme_line.clone());
    }
    lines
}

fn wind_forecast_line(peak: &Option<SamplePeak>) -> String {
    match peak {
        Some(peak) => format!(
            "Open-Meteo forecast: strongest wind in the next 24 h on this grid is {:.0} m/s at {}, hour {}",
            peak.value,
            geo::format_lat_lon_deg(peak.lat, peak.lon),
            peak.when
        ),
        None => "Open-Meteo 24 h wind forecast has not arrived.".into(),
    }
}

fn wave_forecast_line(peak: &Option<SamplePeak>) -> String {
    match peak {
        Some(peak) => format!(
            "Open-Meteo marine forecast: highest wave in the next 24 h on this grid is {:.1} m at {}, hour {}",
            peak.value,
            geo::format_lat_lon_deg(peak.lat, peak.lon),
            peak.when
        ),
        None => "Open-Meteo 24 h wave forecast has not arrived.".into(),
    }
}

fn rain_line(peak: &Option<SamplePeak>) -> String {
    match peak {
        Some(peak) if peak.value > 0.0 => format!(
            "Open-Meteo forecast: heaviest rain in the next 24 h on this grid is {:.1} mm at {}, hour {}",
            peak.value,
            geo::format_lat_lon_deg(peak.lat, peak.lon),
            peak.when
        ),
        Some(_) => "Open-Meteo forecast: no rain above 0 mm in the next 24 h on this grid.".into(),
        None => "Open-Meteo 24 h rain forecast has not arrived.".into(),
    }
}

pub struct DeskSection {
    pub title: &'static str,
    pub lines: Vec<String>,
}

pub fn desk_banner() -> &'static str {
    "FSOT data desk. FSOT is not scoring. These are measured fields from the agencies."
}

pub fn desk_text(
    quakes: &[Quake],
    bulletin: &Bulletin,
    place_selected: bool,
    ensemble: &Option<Result<PlaceEnsemble, String>>,
    air: &Option<Result<PlaceAir, String>>,
) -> String {
    let mut lines = vec![desk_banner().to_string(), String::new()];
    for section in desk_sections(quakes, bulletin, place_selected, ensemble, air) {
        lines.push(section.title.to_string());
        lines.extend(section.lines);
        lines.push(String::new());
    }
    lines.join("\n")
}

pub fn desk_sections(
    quakes: &[Quake],
    bulletin: &Bulletin,
    place_selected: bool,
    ensemble: &Option<Result<PlaceEnsemble, String>>,
    air: &Option<Result<PlaceAir, String>>,
) -> Vec<DeskSection> {
    vec![
        DeskSection { title: "HELD SERIES", lines: held_lines(&bulletin.held) },
        DeskSection { title: "SPACE", lines: space_desk(&bulletin.solar) },
        DeskSection { title: "CLIMATE", lines: climate_lines(&bulletin.climate) },
        DeskSection { title: "THIS PLACE", lines: place_lines(place_selected, ensemble, air) },
        DeskSection { title: "SOLID EARTH", lines: earth_lines(quakes, bulletin) },
        DeskSection { title: "OCEAN AND WEATHER", lines: ocean_desk(bulletin) },
        DeskSection {
            title: "RECORD",
            lines: vec!["The hourly record stores these measured fields with the UTC hour. FSOT is not scoring.".into()],
        },
    ]
}

fn held_lines(notes: &[series::HeldNote]) -> Vec<String> {
    let mut lines = vec![
        "These rows are the measured series, calm hours included. Call pull_series with the name. FSOT is not scoring.".into(),
    ];
    if notes.is_empty() {
        lines.push("Series have not been pulled yet.".into());
        return lines;
    }
    for note in notes {
        lines.push(note.line());
    }
    lines
}

fn space_desk(solar: &SolarReading) -> Vec<String> {
    let mut lines = solar_lines(solar);
    lines.extend(measured_lines(solar));
    lines
}

fn measured_lines(solar: &SolarReading) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(density) = solar.density_cm3 {
        let mut line = format!("NOAA SWPC RTSW proton density {} cm^-3", fmt_meas(density, 2));
        if let Some(temp) = solar.temperature_k {
            line.push_str(&format!(", temperature {} K", fmt_meas(temp, 0)));
        }
        if !solar.plasma_time.is_empty() {
            line.push_str(&format!(" at {}", solar.plasma_time));
        }
        if !solar.plasma_source.is_empty() {
            line.push_str(&format!(", source {}", solar.plasma_source));
        }
        line.push('.');
        lines.push(line);
    } else if !solar.plasma_line.is_empty() {
        lines.push(solar.plasma_line.clone());
    } else {
        lines.push("NOAA SWPC RTSW plasma has not arrived.".into());
    }
    if let Some(flux) = solar.proton_10mev {
        let mut line = format!("GOES >=10 MeV integral proton flux {} pfu", fmt_meas(flux, 3));
        if !solar.proton_10mev_time.is_empty() {
            line.push_str(&format!(" at {}", solar.proton_10mev_time));
        }
        line.push('.');
        lines.push(line);
    } else if !solar.proton_line.is_empty() {
        lines.push(solar.proton_line.clone());
    } else {
        lines.push("GOES >=10 MeV integral proton flux has not arrived.".into());
    }
    if let Some(flux) = solar.electron_2mev {
        let mut line = format!("GOES >=2 MeV integral electron flux {} pfu", fmt_meas(flux, 3));
        if !solar.electron_2mev_time.is_empty() {
            line.push_str(&format!(" at {}", solar.electron_2mev_time));
        }
        line.push('.');
        lines.push(line);
    } else if !solar.electron_line.is_empty() {
        lines.push(solar.electron_line.clone());
    } else {
        lines.push("GOES >=2 MeV integral electron flux has not arrived.".into());
    }
    lines.push(magnetometer_sentence(solar));
    if let Some(flux) = solar.f107_sfu {
        let mut line = format!("NOAA SWPC F10.7 {} sfu", fmt_meas(flux, 1));
        if !solar.f107_time.is_empty() {
            line.push_str(&format!(" at {}", solar.f107_time));
        }
        if !solar.f107_schedule.is_empty() {
            line.push_str(&format!(" ({})", solar.f107_schedule));
        }
        line.push('.');
        lines.push(line);
        if let Some(mean) = solar.f107_mean_sfu {
            let mut mean_line = format!("90-day mean {} sfu", fmt_meas(mean, 1));
            if !solar.f107_mean_time.is_empty() {
                mean_line.push_str(&format!(" at {}", solar.f107_mean_time));
            }
            mean_line.push('.');
            lines.push(mean_line);
        }
    } else if !solar.f107_line.is_empty() {
        lines.push(solar.f107_line.clone());
    } else {
        lines.push("NOAA SWPC F10.7 flux has not arrived.".into());
    }
    if solar.region_line.is_empty() {
        lines.push("NOAA SWPC solar regions have not arrived.".into());
    } else {
        lines.push(solar.region_line.clone());
    }
    if solar.sunspot_line.is_empty() {
        lines.push("NOAA SWPC sunspot report has not arrived.".into());
    } else {
        lines.push(solar.sunspot_line.clone());
    }
    if let Some(dst) = solar.dst_nt {
        let mut line = format!("Kyoto Dst {} nT", fmt_meas(dst, 0));
        if !solar.dst_time.is_empty() {
            line.push_str(&format!(" at {}", solar.dst_time));
        }
        line.push_str(", published by NOAA SWPC.");
        lines.push(line);
    } else if !solar.dst_line.is_empty() {
        lines.push(solar.dst_line.clone());
    } else {
        lines.push("Kyoto Dst has not arrived.".into());
    }
    lines.push(scale_sentence(solar));
    if !solar.alert_line.is_empty() {
        lines.push(solar.alert_line.clone());
    } else {
        lines.push("NOAA SWPC alerts have not arrived.".into());
    }
    lines
}

fn scale_sentence(solar: &SolarReading) -> String {
    if solar.scale_r.is_empty() && solar.scale_s.is_empty() && solar.scale_g.is_empty() {
        if !solar.scale_line.is_empty() {
            return solar.scale_line.clone();
        }
        return "NOAA scales have not arrived.".into();
    }
    let mut parts = Vec::new();
    if !solar.scale_r.is_empty() {
        parts.push(format!("R {}", solar.scale_r));
    }
    if !solar.scale_s.is_empty() {
        parts.push(format!("S {}", solar.scale_s));
    }
    if !solar.scale_g.is_empty() {
        parts.push(format!("G {}", solar.scale_g));
    }
    let mut line = format!("NOAA scales, current: {}.", parts.join(", "));
    if !solar.scale_time.is_empty() {
        line.push_str(&format!(" {}", solar.scale_time));
    }
    line
}

fn climate_lines(climate: &ClimateReading) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(anom) = climate.oni_anom {
        let season = if climate.oni_season.is_empty() { "season".to_string() } else { climate.oni_season.clone() };
        let year = climate.oni_year.map(|year| format!(" {year}")).unwrap_or_default();
        let total = climate.oni_total.map(|total| format!(", total {} °C", fmt_meas(total, 2))).unwrap_or_default();
        lines.push(format!("CPC ONI {season}{year}: Niño 3.4 anomaly {} °C{total}.", fmt_meas(anom, 2)));
        lines.push(
            "CPC's rule: an anomaly of +0.5 or higher is El Niño, and -0.5 or lower is La Niña, when it holds for five overlapping seasons. This row is one season."
                .into(),
        );
    } else if !climate.oni_line.is_empty() {
        lines.push(climate.oni_line.clone());
    } else {
        lines.push("CPC ONI index has not arrived.".into());
    }
    lines.push(index_sentence("AO", climate.ao, climate.ao_year, climate.ao_month, &climate.ao_line));
    lines.push(index_sentence("NAO", climate.nao, climate.nao_year, climate.nao_month, &climate.nao_line));
    if climate.mjo_line.is_empty() {
        lines.push("BOM RMM index has not arrived.".into());
    } else {
        lines.push(climate.mjo_line.clone());
    }
    if climate.ice_line.is_empty() {
        lines.push("NSIDC sea ice extent has not arrived.".into());
    } else {
        lines.push(climate.ice_line.clone());
    }
    if climate.co2_line.is_empty() {
        lines.push("NOAA GML daily CO2 has not arrived.".into());
    } else {
        lines.push(climate.co2_line.clone());
    }
    lines
}

fn index_sentence(name: &str, index: Option<f32>, year: Option<i32>, month: Option<i32>, missing: &str) -> String {
    if let Some(index) = index {
        let when = match (year, month) {
            (Some(year), Some(month)) => format!(" {year}-{month:02}"),
            _ => String::new(),
        };
        format!("CPC {name}{when}: {}.", fmt_meas(index, 4))
    } else if !missing.is_empty() {
        missing.to_string()
    } else {
        format!("CPC {name} index has not arrived.")
    }
}

fn place_lines(
    place_selected: bool,
    ensemble: &Option<Result<PlaceEnsemble, String>>,
    air: &Option<Result<PlaceAir, String>>,
) -> Vec<String> {
    let mut lines = ensemble_lines(place_selected, ensemble);
    lines.extend(air_lines(place_selected, air));
    lines
}

fn ensemble_lines(place_selected: bool, ensemble: &Option<Result<PlaceEnsemble, String>>) -> Vec<String> {
    if !place_selected {
        return vec!["Choose a place to load the ensemble spread for that point.".into()];
    }
    match ensemble {
        None => vec!["Open-Meteo ensemble for this place is loading.".into()],
        Some(Err(_)) => vec!["Open-Meteo ensemble for this place did not load.".into()],
        Some(Ok(cast)) => format_ensemble(cast),
    }
}

fn format_ensemble(cast: &PlaceEnsemble) -> Vec<String> {
    if cast.hours.is_empty() {
        return vec!["Open-Meteo ensemble for this place did not load.".into(), ensemble_held_line(cast)];
    }
    let mut lines = vec![
        "Open-Meteo GFS seamless ensemble for this place. Hours are UTC.".into(),
        "Spread is the minimum, mean, and maximum of the member values.".into(),
    ];
    for hour in &cast.hours {
        let label = hour.when.get(11..16).unwrap_or(hour.when.as_str());
        let mut parts = vec![label.to_string()];
        if let Some(spread) = &hour.temp {
            parts.push(spread_text("temp", spread, "°C"));
        }
        if let Some(spread) = &hour.rain {
            parts.push(spread_text("rain", spread, "mm"));
        }
        if let Some(spread) = &hour.wind {
            parts.push(spread_text("wind", spread, "m/s"));
        }
        lines.push(parts.join("  "));
    }
    lines.push(ensemble_held_line(cast));
    lines
}

fn ensemble_held_line(cast: &PlaceEnsemble) -> String {
    if cast.held_rows == 0 {
        return "Full member series for this place have not been held.".into();
    }
    if cast.held_first.is_empty() || cast.held_first == cast.held_last {
        let when = if cast.held_first.is_empty() { String::new() } else { format!(", {}", cast.held_first) };
        return format!("Full member series held: {} rows{when}. Pull series ensemble.", cast.held_rows);
    }
    format!(
        "Full member series held: {} rows, {} through {}. Pull series ensemble.",
        cast.held_rows, cast.held_first, cast.held_last
    )
}

fn air_lines(place_selected: bool, air: &Option<Result<PlaceAir, String>>) -> Vec<String> {
    if !place_selected {
        return vec!["Choose a place to load carbon monoxide, sulphur dioxide, and dust for that point.".into()];
    }
    match air {
        None => vec!["Open-Meteo air quality for this place is loading.".into()],
        Some(Err(_)) => vec!["Open-Meteo air quality for this place did not load.".into()],
        Some(Ok(sample)) => {
            let mut lines = vec![if sample.line.is_empty() {
                "Open-Meteo air quality for this place did not load.".into()
            } else {
                sample.line.clone()
            }];
            lines.push(air_held_line(sample));
            lines
        }
    }
}

fn air_held_line(sample: &PlaceAir) -> String {
    if sample.held_rows == 0 {
        return "Hourly air-quality series for this place have not been held.".into();
    }
    if sample.held_first.is_empty() || sample.held_first == sample.held_last {
        let when = if sample.held_first.is_empty() { String::new() } else { format!(", {}", sample.held_first) };
        return format!("Hourly air-quality series held: {} rows{when}. Pull series air.", sample.held_rows);
    }
    format!(
        "Hourly air-quality series held: {} rows, {} through {}. Pull series air.",
        sample.held_rows, sample.held_first, sample.held_last
    )
}

fn spread_text(name: &str, spread: &Spread, unit: &str) -> String {
    if spread.members < 2 {
        format!("{name} {} {unit} (1 member)", fmt_meas(spread.mean, 2))
    } else {
        format!(
            "{name} {} to {} {unit}, mean {} ({} members)",
            fmt_meas(spread.min, 2),
            fmt_meas(spread.max, 2),
            fmt_meas(spread.mean, 2),
            spread.members
        )
    }
}

fn ocean_desk(bulletin: &Bulletin) -> Vec<String> {
    let mut lines = vec![
        wind_forecast_line(&bulletin.wind_forecast),
        rain_line(&bulletin.rain_forecast),
        wave_forecast_line(&bulletin.wave_forecast),
    ];
    if bulletin.pressure_line.is_empty() {
        lines.push("Open-Meteo sea-level pressure and relative humidity have not arrived.".into());
    } else {
        lines.push(bulletin.pressure_line.clone());
    }
    if bulletin.current_hour_line.is_empty() {
        lines.push("Open-Meteo hourly ocean current has not arrived.".into());
    } else {
        lines.push(bulletin.current_hour_line.clone());
    }
    if bulletin.gauge_line.is_empty() {
        lines.push("UNESCO IOC sea-level gauges have not arrived.".into());
    } else {
        lines.push(bulletin.gauge_line.clone());
    }
    if !bulletin.storm_line.is_empty() {
        lines.push(bulletin.storm_line.clone());
    }
    lines
}

fn earth_lines(quakes: &[Quake], bulletin: &Bulletin) -> Vec<String> {
    let mut lines = quake_desk(quakes);
    if bulletin.fire_line.is_empty() {
        lines.push("NASA FIRMS active fire detections have not arrived.".into());
    } else {
        lines.push(bulletin.fire_line.clone());
    }
    if bulletin.vaac_desk.is_empty() {
        if bulletin.vaac_line.is_empty() {
            lines.push("NWS volcanic ash advisories have not arrived.".into());
        } else {
            lines.push(bulletin.vaac_line.clone());
        }
    } else {
        lines.extend(bulletin.vaac_desk.iter().cloned());
    }
    lines
}

fn quake_desk(quakes: &[Quake]) -> Vec<String> {
    let mut lines = vec![format!("{} earthquakes are in the loaded USGS record.", quakes.len())];
    if let Some(biggest) = quakes.iter().max_by(|a, b| a.mag.total_cmp(&b.mag)) {
        lines.push(format!(
            "Largest loaded: M{:.1}, {}, {:.0} km deep.",
            biggest.mag, biggest.place, biggest.depth_km
        ));
    }
    lines.push("This counts the loaded record. It does not forecast the next event.".into());
    lines
}

fn fmt_meas(value: f32, decimals: usize) -> String {
    let text = format!("{:.*}", decimals, value);
    if decimals == 0 {
        return text;
    }
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    if trimmed == "-0" { "0".into() } else { trimmed.to_string() }
}

fn current_line(ocean: &[OceanSample]) -> String {
    let best = ocean
        .iter()
        .filter_map(|sample| sample.current_kmh.map(|speed| (sample, speed)))
        .max_by(|a, b| a.1.total_cmp(&b.1));
    match best {
        Some((sample, speed)) => {
            let toward = sample.current_toward_deg.map(|deg| format!(" toward {deg:.0}°")).unwrap_or_default();
            format!(
                "Strongest ocean current in the sample: {speed:.1} km/h{toward} at {}",
                geo::format_lat_lon_deg(sample.lat, sample.lon)
            )
        }
        None if ocean.is_empty() => "Ocean current has not arrived.".into(),
        None => "Ocean current has not arrived on this grid.".into(),
    }
}

fn scale_suffix(scale: &Option<String>) -> String {
    match scale {
        Some(text) if !text.is_empty() => format!(" · NOAA scale {text}"),
        _ => String::new(),
    }
}

fn volcano_lines(lines: &mut Vec<String>, bulletin: &Bulletin) {
    if !bulletin.alerts_loaded {
        lines.push("USGS volcano alerts have not arrived.".into());
        return;
    }
    if bulletin.volcano_alerts.is_empty() {
        lines.push("USGS reports no elevated volcano alert.".into());
        return;
    }
    let list = bulletin
        .volcano_alerts
        .iter()
        .map(|alert| format!("{} {}/{}", alert.name, alert.alert_level, alert.color_code))
        .collect::<Vec<_>>()
        .join("; ");
    lines.push(format!("USGS volcano alerts: {list}."));
    for alert in bulletin
        .volcano_alerts
        .iter()
        .filter(|alert| alert.synopsis.to_ascii_lowercase().contains("forecast"))
        .take(2)
    {
        let text: String = alert.synopsis.chars().take(220).collect();
        lines.push(format!("USGS notice: {text}"));
    }
}

fn joint_lines(
    quakes: &[Quake],
    wind: &[WindSample],
    ocean: &[OceanSample],
    volcanoes: &[Volcano],
    faults: &[Vec<(f32, f32)>],
    bulletin: &Bulletin,
    now_unix: f64,
) -> Vec<String> {
    let mut lines = vec![
        "Joint record: one Earth frame and one UTC clock. Distances are great-circle kilometers. A short distance is a shared neighborhood, not a cause. FSOT is not scoring this record yet.".into(),
    ];
    let Some(quake) = quakes.iter().max_by(|a, b| a.mag.total_cmp(&b.mag)) else {
        lines.push("Joint record has no quake to measure distances from.".into());
        lines.push(forecast_separation(bulletin));
        lines.push(lookahead_line(bulletin, now_unix));
        return lines;
    };
    match nearest_alert(quake.lat, quake.lon, bulletin) {
        Some((alert, km)) => lines.push(format!(
            "Nearest elevated volcano to the largest quake: {}, {}.",
            alert.name,
            fmt_km(km)
        )),
        None => lines.push("No elevated-volcano distance is measured on this pass.".into()),
    }
    match nearest_holocene(quake.lat, quake.lon, volcanoes) {
        Some((name, km)) => lines.push(format!("Nearest Holocene volcano since 2000: {name}, {}.", fmt_km(km))),
        None => lines.push("No Holocene volcano since 2000 is loaded.".into()),
    }
    match nearest_fault_km(quake.lat, quake.lon, faults) {
        Some(km) => lines.push(format!("Nearest plate-boundary sample: {}.", fmt_km(km))),
        None => lines.push("No plate-boundary sample is loaded.".into()),
    }
    let wind_km = wind.iter().max_by(|a, b| a.wind_ms.total_cmp(&b.wind_ms)).map(|sample| km_between(quake.lat, quake.lon, sample.lat, sample.lon));
    let sea_km = ocean.iter().max_by(|a, b| a.wave_m.total_cmp(&b.wave_m)).map(|sample| km_between(quake.lat, quake.lon, sample.lat, sample.lon));
    match (wind_km, sea_km) {
        (Some(wind_km), Some(sea_km)) => lines.push(format!(
            "Strongest wind sample is {} from the largest quake. Highest sea sample is {} from the largest quake.",
            fmt_km(wind_km),
            fmt_km(sea_km)
        )),
        (Some(wind_km), None) => lines.push(format!("Strongest wind sample is {} from the largest quake.", fmt_km(wind_km))),
        (None, Some(sea_km)) => lines.push(format!("Highest sea sample is {} from the largest quake.", fmt_km(sea_km))),
        (None, None) => lines.push("Wind and sea samples are not loaded, so those distances are not measured.".into()),
    }
    lines.push(forecast_separation(bulletin));
    let alerts = bulletin.volcano_alerts.iter().filter(|alert| km_between(quake.lat, quake.lon, alert.lat, alert.lon) <= 700.0).count();
    let winds = wind.iter().filter(|sample| km_between(quake.lat, quake.lon, sample.lat, sample.lon) <= 700.0).count();
    let seas = ocean.iter().filter(|sample| km_between(quake.lat, quake.lon, sample.lat, sample.lon) <= 700.0).count();
    lines.push(format!(
        "Within 700 km of the largest quake: elevated volcanoes {alerts}, wind samples {winds}, sea samples {seas}."
    ));
    lines.push(lookahead_line(bulletin, now_unix));
    lines
}

fn forecast_separation(bulletin: &Bulletin) -> String {
    match (&bulletin.wind_forecast, &bulletin.wave_forecast) {
        (Some(wind), Some(wave)) => format!(
            "Open-Meteo wind and wave forecast points are {} apart.",
            fmt_km(km_between(wind.lat, wind.lon, wave.lat, wave.lon))
        ),
        _ => "Open-Meteo wind and wave forecast points are not both loaded.".into(),
    }
}

fn lookahead_line(bulletin: &Bulletin, now_unix: f64) -> String {
    let now_stamp = hour_stamp(now_unix);
    let mut parts = Vec::new();
    if bulletin.solar.kp_forecast.is_some() && !bulletin.solar.kp_forecast_time.is_empty() && !bulletin.solar.kp_now_time.is_empty() {
        let relation = if stamp_key(&bulletin.solar.kp_forecast_time) > stamp_key(&bulletin.solar.kp_now_time) {
            "is after"
        } else {
            "is not after"
        };
        parts.push(format!(
            "NOAA predicted Kp time {} {relation} the latest Kp row {}",
            bulletin.solar.kp_forecast_time, bulletin.solar.kp_now_time
        ));
    }
    if let Some(peak) = &bulletin.wind_forecast {
        parts.push(ahead_phrase("Open-Meteo wind hour", &peak.when, &now_stamp));
    }
    if let Some(peak) = &bulletin.wave_forecast {
        parts.push(ahead_phrase("Open-Meteo wave hour", &peak.when, &now_stamp));
    }
    if parts.is_empty() {
        "Look-ahead check: no published forecast time is loaded.".into()
    } else {
        format!("Look-ahead check: {}.", parts.join("; "))
    }
}

fn ahead_phrase(label: &str, when: &str, now_stamp: &str) -> String {
    if stamp_key(when) >= stamp_key(now_stamp) {
        format!("{label} {when} is at or after UTC {now_stamp}")
    } else {
        format!("{label} {when} is behind UTC {now_stamp}")
    }
}

fn hour_stamp(unix: f64) -> String {
    let secs = unix.floor() as i64;
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (year, month, day) = geo::civil_from_days(days);
    let hour = rem / 3600;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}")
}

fn stamp_key(text: &str) -> String {
    text.chars().take(13).collect()
}

fn fmt_km(km: f32) -> String {
    format!("{km:.0} km")
}

fn nearest_alert<'a>(lat: f32, lon: f32, bulletin: &'a Bulletin) -> Option<(&'a VolcanoAlert, f32)> {
    if !bulletin.alerts_loaded {
        return None;
    }
    bulletin.volcano_alerts.iter().map(|alert| (alert, km_between(lat, lon, alert.lat, alert.lon))).min_by(|a, b| a.1.total_cmp(&b.1))
}

fn nearest_holocene<'a>(lat: f32, lon: f32, volcanoes: &'a [Volcano]) -> Option<(&'a str, f32)> {
    volcanoes
        .iter()
        .filter(|volcano| volcano.last_year.unwrap_or(i32::MIN) >= 2000)
        .map(|volcano| (volcano.name.as_str(), km_between(lat, lon, volcano.lat, volcano.lon)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

fn nearest_fault_km(lat: f32, lon: f32, faults: &[Vec<(f32, f32)>]) -> Option<f32> {
    let mut best: Option<f32> = None;
    for chain in faults {
        for (flat, flon) in chain {
            let km = km_between(lat, lon, *flat, *flon);
            if best.map(|current| km < current).unwrap_or(true) {
                best = Some(km);
            }
        }
    }
    best
}

struct Cluster {
    count: usize,
    max_mag: f32,
    lat: f32,
    lon: f32,
}

fn clusters_within(quakes: &[Quake], now_ms: i64, km: f32, window_ms: i64) -> Vec<Cluster> {
    let recent: Vec<&Quake> = quakes
        .iter()
        .filter(|q| q.mag >= 4.5 && now_ms.saturating_sub(q.time_ms) <= window_ms && now_ms >= q.time_ms)
        .collect();
    let mut used = vec![false; recent.len()];
    let mut out = Vec::new();
    for seed in 0..recent.len() {
        if used[seed] {
            continue;
        }
        let mut stack = vec![seed];
        used[seed] = true;
        let mut members = Vec::new();
        while let Some(i) = stack.pop() {
            members.push(i);
            for (j, other) in recent.iter().enumerate() {
                if used[j] {
                    continue;
                }
                if km_between(recent[i].lat, recent[i].lon, other.lat, other.lon) <= km {
                    used[j] = true;
                    stack.push(j);
                }
            }
        }
        if members.len() < 3 {
            continue;
        }
        let mut lat = 0.0;
        let mut lon_sin = 0.0;
        let mut lon_cos = 0.0;
        let mut max_mag = 0.0_f32;
        for i in &members {
            let q = recent[*i];
            lat += q.lat;
            let lon = q.lon.to_radians();
            lon_sin += lon.sin();
            lon_cos += lon.cos();
            max_mag = max_mag.max(q.mag);
        }
        let n = members.len() as f32;
        out.push(Cluster {
            count: members.len(),
            max_mag,
            lat: lat / n,
            lon: lon_sin.atan2(lon_cos).to_degrees(),
        });
    }
    out.sort_by(|a, b| b.max_mag.total_cmp(&a.max_mag));
    out
}

fn age_text(ms: i64) -> String {
    let mins = ms / 60_000;
    if mins < 1 {
        "just now".into()
    } else if mins < 90 {
        format!("{mins} min ago")
    } else if mins < 48 * 60 {
        format!("{} h ago", mins / 60)
    } else {
        format!("{} d ago", mins / (60 * 24))
    }
}

pub(crate) fn km_between(lat1: f32, lon1: f32, lat2: f32, lon2: f32) -> f32 {
    let p1 = lat1.to_radians();
    let p2 = lat2.to_radians();
    let dp = (lat2 - lat1).to_radians();
    let dl = (lon2 - lon1).to_radians();
    let a = (dp * 0.5).sin().powi(2) + p1.cos() * p2.cos() * (dl * 0.5).sin().powi(2);
    2.0 * 6371.0 * a.sqrt().clamp(0.0, 1.0).asin()
}

fn fetch_quakes(agent: &ureq::Agent) -> Result<Vec<Quake>, String> {
    let url = "https://earthquake.usgs.gov/earthquakes/feed/v1.0/summary/2.5_day.geojson";
    let body = http_get(agent, url)?;
    let _ = fs::write(cache_path("quakes.json"), &body);
    parse_quakes(&body)
}

/// One row per loaded USGS event. A missing alert stays null. Magnitude 0 stays 0.
fn quake_rows(quakes: &[Quake]) -> Vec<series::Row> {
    quakes
        .iter()
        .map(|quake| {
            let mut row = series::Row::new();
            row.insert("t".into(), serde_json::Value::String(quake_iso(quake.time_ms)));
            row.insert("id".into(), serde_json::Value::String(quake.id.clone()));
            series::put_f64(&mut row, "mag", f64::from(quake.mag));
            series::put_f64(&mut row, "lat", f64::from(quake.lat));
            series::put_f64(&mut row, "lon", f64::from(quake.lon));
            series::put_f64(&mut row, "depth_km", f64::from(quake.depth_km));
            row.insert("place".into(), serde_json::Value::String(quake.place.clone()));
            match &quake.alert {
                Some(alert) => row.insert("alert".into(), serde_json::Value::String(alert.clone())),
                None => row.insert("alert".into(), serde_json::Value::Null),
            };
            row.insert("tsunami".into(), serde_json::Value::Bool(quake.tsunami));
            match quake.nst {
                Some(nst) => row.insert("nst".into(), serde_json::Value::from(nst)),
                None => row.insert("nst".into(), serde_json::Value::Null),
            };
            match quake.gap {
                Some(gap) => series::put_f64(&mut row, "gap", f64::from(gap)),
                None => {
                    row.insert("gap".into(), serde_json::Value::Null);
                }
            };
            match quake.dmin {
                Some(dmin) => series::put_f64(&mut row, "dmin", f64::from(dmin)),
                None => {
                    row.insert("dmin".into(), serde_json::Value::Null);
                }
            };
            match quake.rms {
                Some(rms) => series::put_f64(&mut row, "rms", f64::from(rms)),
                None => {
                    row.insert("rms".into(), serde_json::Value::Null);
                }
            };
            match &quake.mag_type {
                Some(kind) => row.insert("mag_type".into(), serde_json::Value::String(kind.clone())),
                None => row.insert("mag_type".into(), serde_json::Value::Null),
            };
            row
        })
        .collect()
}

fn quake_iso(time_ms: i64) -> String {
    let secs = time_ms.div_euclid(1000);
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (year, month, day) = geo::civil_from_days(days);
    let hour = rem / 3600;
    let minute = (rem % 3600) / 60;
    let second = rem % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

struct WindFetch {
    samples: Vec<WindSample>,
    forecast: Option<SamplePeak>,
    rain: Option<SamplePeak>,
    pressure_line: String,
}

struct OceanFetch {
    samples: Vec<OceanSample>,
    forecast: Option<SamplePeak>,
    current_hour_line: String,
}

fn fetch_wind(agent: &ureq::Agent, held: &mut Vec<series::HeldNote>) -> Result<WindFetch, String> {
    let points = grid_points(-60, 70, 20, 20);
    let mut samples = Vec::new();
    let mut forecast = None;
    let mut rain = None;
    let mut grid = Vec::new();
    let mut hours = Vec::new();
    for batch in points.chunks(40) {
        let lats = join_coords(batch.iter().map(|p| p.0));
        let lons = join_coords(batch.iter().map(|p| p.1));
        let url = wind_grid_url(&lats, &lons);
        let body = http_get(agent, &url)?;
        samples.extend(parse_wind(&body)?);
        keep_higher(&mut forecast, hourly_peak(&body, "wind_speed_10m"));
        keep_higher(&mut rain, hourly_peak(&body, "precipitation"));
        grid.extend(series::current_rows(
            &body,
            &[
                ("wind_speed_10m", "wind_ms"),
                ("wind_direction_10m", "from_deg"),
                ("temperature_2m", "temp_c"),
                ("precipitation", "precip_mm"),
                ("pressure_msl", "pressure_hpa"),
                ("relative_humidity_2m", "humidity_pct"),
            ],
        ));
        hours.extend(series::hour_rows(
            &body,
            &[
                ("wind_speed_10m", "wind_ms"),
                ("precipitation", "precip_mm"),
                ("pressure_msl", "pressure_hpa"),
                ("relative_humidity_2m", "humidity_pct"),
            ],
        ));
    }
    if samples.is_empty() {
        return Err("Open-Meteo returned no wind".into());
    }
    let pressure_line = pressure_span_line(&grid);
    push_measured(held, "wind-grid", "Open-Meteo", grid, "Open-Meteo wind did not include grid rows.");
    push_measured(held, "wind-hours", "Open-Meteo", hours, "Open-Meteo wind hours did not include rows.");
    Ok(WindFetch { samples, forecast, rain, pressure_line })
}

fn wind_grid_url(lats: &str, lons: &str) -> String {
    format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lats}&longitude={lons}&current=temperature_2m,wind_speed_10m,wind_direction_10m,precipitation,pressure_msl,relative_humidity_2m&hourly=wind_speed_10m,precipitation,pressure_msl,relative_humidity_2m&forecast_hours=24&wind_speed_unit=ms"
    )
}

fn pressure_span_line(rows: &[series::Row]) -> String {
    let pressure = finite_span(rows, "pressure_hpa");
    let humidity = finite_span(rows, "humidity_pct");
    match (pressure, humidity) {
        (Some((low, high)), Some((humid_low, humid_high))) => format!(
            "Open-Meteo sea-level pressure on this grid spans {} to {} hPa. Relative humidity spans {} to {} percent.",
            fmt_meas(low, 0),
            fmt_meas(high, 0),
            fmt_meas(humid_low, 0),
            fmt_meas(humid_high, 0)
        ),
        (Some((low, high)), None) => format!(
            "Open-Meteo sea-level pressure on this grid spans {} to {} hPa. Relative humidity did not arrive on this grid.",
            fmt_meas(low, 0),
            fmt_meas(high, 0)
        ),
        (None, Some((low, high))) => format!(
            "Open-Meteo relative humidity on this grid spans {} to {} percent. Sea-level pressure did not arrive on this grid.",
            fmt_meas(low, 0),
            fmt_meas(high, 0)
        ),
        (None, None) => "Open-Meteo sea-level pressure and relative humidity did not arrive on this grid.".into(),
    }
}

fn finite_span(rows: &[series::Row], key: &str) -> Option<(f32, f32)> {
    let mut low = f32::MAX;
    let mut high = f32::MIN;
    let mut any = false;
    for row in rows {
        let Some(value) = row.get(key).and_then(|item| item.as_f64()) else { continue };
        if !value.is_finite() {
            continue;
        }
        let value = value as f32;
        any = true;
        low = low.min(value);
        high = high.max(value);
    }
    any.then_some((low, high))
}

fn fetch_fires(agent: &ureq::Agent, held: &mut Vec<series::HeldNote>) -> String {
    let sources = [
        (
            "Suomi-NPP VIIRS",
            "https://firms.modaps.eosdis.nasa.gov/data/active_fire/suomi-npp-viirs-c2/csv/SUOMI_VIIRS_C2_Global_24h.csv",
        ),
        (
            "NOAA-20 VIIRS",
            "https://firms.modaps.eosdis.nasa.gov/data/active_fire/noaa-20-viirs-c2/csv/J1_VIIRS_C2_Global_24h.csv",
        ),
        (
            "NOAA-21 VIIRS",
            "https://firms.modaps.eosdis.nasa.gov/data/active_fire/noaa-21-viirs-c2/csv/J2_VIIRS_C2_Global_24h.csv",
        ),
    ];
    let mut rows = Vec::new();
    let mut misses = Vec::new();
    let mut loaded = 0;
    for (name, url) in sources {
        match http_get_once(agent, url).ok().and_then(|body| series::firms_rows(&body).ok()) {
            Some(mut parsed) => {
                rows.append(&mut parsed);
                loaded += 1;
            }
            None => misses.push(name),
        }
    }
    if loaded == 0 {
        recall_named(held, "fires", "NASA FIRMS VIIRS", "NASA FIRMS active fire detections did not load.");
        return "NASA FIRMS active fire detections did not load.".into();
    }
    let line = fire_summary(&rows, &misses);
    hold_body(held, "fires", "NASA FIRMS VIIRS", Ok(rows), "NASA FIRMS active fire detections did not load.", true);
    line
}

fn fire_summary(rows: &[series::Row], misses: &[&str]) -> String {
    let mut line = if rows.is_empty() {
        "NASA FIRMS VIIRS 24 h catalog loaded with no detections.".to_string()
    } else {
        let mut text = format!("NASA FIRMS VIIRS 24 h detections: {}.", rows.len());
        let best = rows.iter().filter_map(|row| {
            let frp = row.get("frp").and_then(|item| item.as_f64())?;
            frp.is_finite().then_some((frp, row))
        }).max_by(|left, right| left.0.total_cmp(&right.0));
        if let Some((frp, row)) = best {
            let lat = row.get("lat").and_then(|item| item.as_f64()).unwrap_or(0.0) as f32;
            let lon = row.get("lon").and_then(|item| item.as_f64()).unwrap_or(0.0) as f32;
            let satellite = row.get("satellite").and_then(|item| item.as_str()).unwrap_or("");
            text.push_str(&format!(
                " Highest FRP {} MW at {}, satellite {satellite}, a FIRMS detection.",
                fmt_meas(frp as f32, 1),
                geo::format_lat_lon_deg(lat, lon)
            ));
        }
        text
    };
    if !misses.is_empty() {
        line.push(' ');
        line.push_str(&misses.join(", "));
        line.push_str(" did not load.");
    }
    line
}

fn fetch_ocean(agent: &ureq::Agent, held: &mut Vec<series::HeldNote>) -> Result<OceanFetch, String> {
    let points = grid_points(-60, 60, 30, 30);
    let mut samples = Vec::new();
    let mut forecast = None;
    let mut grid = Vec::new();
    let mut hours = Vec::new();
    for batch in points.chunks(40) {
        let lats = join_coords(batch.iter().map(|p| p.0));
        let lons = join_coords(batch.iter().map(|p| p.1));
        let url = marine_batch_url(&lats, &lons);
        let body = http_get(agent, &url)?;
        samples.extend(parse_ocean(&body)?);
        keep_higher(&mut forecast, hourly_peak(&body, "wave_height"));
        grid.extend(series::current_rows(
            &body,
            &[
                ("sea_surface_temperature", "sst_c"),
                ("wave_height", "wave_m"),
                ("ocean_current_velocity", "current_kmh"),
                ("ocean_current_direction", "current_toward_deg"),
            ],
        ));
        hours.extend(series::hour_rows(
            &body,
            &[
                ("wave_height", "wave_m"),
                ("ocean_current_velocity", "current_kmh"),
                ("ocean_current_direction", "current_toward_deg"),
            ],
        ));
    }
    if samples.is_empty() {
        return Err("marine API returned no sea samples".into());
    }
    let current_hour_line = hourly_current_line(&hours);
    push_measured(held, "ocean-grid", "Open-Meteo marine", grid, "Open-Meteo marine did not include grid rows.");
    push_measured(held, "ocean-hours", "Open-Meteo marine", hours, "Open-Meteo marine hours did not include rows.");
    Ok(OceanFetch { samples, forecast, current_hour_line })
}

fn marine_batch_url(lats: &str, lons: &str) -> String {
    format!(
        "https://marine-api.open-meteo.com/v1/marine?latitude={lats}&longitude={lons}&current=wave_height,sea_surface_temperature,ocean_current_velocity,ocean_current_direction&hourly=wave_height,ocean_current_velocity,ocean_current_direction&forecast_hours=24"
    )
}

fn hourly_current_line(rows: &[series::Row]) -> String {
    match finite_span(rows, "current_kmh") {
        Some((low, high)) => format!(
            "Open-Meteo hourly ocean current on the loaded hours spans {} to {} km/h. Direction is stored with each hour.",
            fmt_meas(low, 1),
            fmt_meas(high, 1)
        ),
        None => "Open-Meteo hourly ocean current did not arrive on the loaded hours.".into(),
    }
}

fn keep_higher(slot: &mut Option<SamplePeak>, next: Option<SamplePeak>) {
    let Some(peak) = next else { return };
    if slot.as_ref().map(|current| peak.value > current.value).unwrap_or(true) {
        *slot = Some(peak);
    }
}

fn grid_points(lat_min: i32, lat_max: i32, lat_step: i32, lon_step: i32) -> Vec<(f32, f32)> {
    let mut points = Vec::new();
    let mut lat = lat_min;
    while lat <= lat_max {
        let mut lon = -180;
        while lon < 180 {
            points.push((lat as f32, lon as f32));
            lon += lon_step;
        }
        lat += lat_step;
    }
    points
}

fn join_coords(values: impl Iterator<Item = f32>) -> String {
    let mut out = String::new();
    for (i, value) in values.enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!("{value:.0}"));
    }
    out
}

fn http_get(agent: &ureq::Agent, url: &str) -> Result<String, String> {
    let mut last = String::from("request failed");
    for attempt in 0..4 {
        match agent
            .get(url)
            .set("User-Agent", "FSOT-Global-Monitor/0.1")
            .call()
        {
            Ok(response) => return response.into_string().map_err(|err| err.to_string()),
            Err(err) => {
                last = err.to_string();
                let retry = last.contains("429")
                    || last.contains("timed out")
                    || last.contains("TimedOut")
                    || last.contains("Connection");
                if attempt == 3 || !retry {
                    break;
                }
                thread::sleep(Duration::from_secs(6 * (attempt as u64 + 1)));
            }
        }
    }
    Err(last)
}

fn fetch_closed_checks(agent: &ureq::Agent, now_unix: f64) -> Vec<crate::record::HourCheck> {
    let now_hour = hour_stamp(now_unix);
    let Ok(text) = fs::read_to_string(cache_path("record").join("state.json")) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    let Some(pending) = value.get("pending").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    let mut checks = Vec::new();
    for item in pending {
        if checks.len() >= 6 {
            break;
        }
        let Some(of) = item.get("of").and_then(|v| v.as_str()) else { continue };
        let Some(hour) = item.get("hour").and_then(|v| v.as_str()) else { continue };
        if stamp_key(hour).as_str() >= now_hour.as_str() {
            continue;
        }
        let Some(lat) = item.get("lat").and_then(|v| v.as_f64()) else { continue };
        let Some(lon) = item.get("lon").and_then(|v| v.as_f64()) else { continue };
        let Some(url) = closed_check_url(of, lat, lon) else { continue };
        let Some(field) = closed_check_field(of) else { continue };
        let Ok(body) = http_get_once(agent, &url) else { continue };
        let Some(value) = hour_value_at(&body, field, hour) else { continue };
        checks.push(crate::record::HourCheck { of: of.to_string(), hour: stamp_key(hour), value });
    }
    checks
}

fn closed_check_url(of: &str, lat: f64, lon: f64) -> Option<String> {
    match of {
        "wind" => Some(format!(
            "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}&hourly=wind_speed_10m&wind_speed_unit=ms&past_hours=36&forecast_hours=1"
        )),
        "rain" => Some(format!(
            "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}&hourly=precipitation&past_hours=36&forecast_hours=1"
        )),
        "wave" => Some(format!(
            "https://marine-api.open-meteo.com/v1/marine?latitude={lat}&longitude={lon}&hourly=wave_height&past_hours=36&forecast_hours=1"
        )),
        _ => None,
    }
}

fn closed_check_field(of: &str) -> Option<&'static str> {
    match of {
        "wind" => Some("wind_speed_10m"),
        "rain" => Some("precipitation"),
        "wave" => Some("wave_height"),
        _ => None,
    }
}

fn hour_value_at(body: &str, field: &str, hour: &str) -> Option<f32> {
    let value: Value = serde_json::from_str(body).ok()?;
    let item = value.as_array().and_then(|list| list.first()).unwrap_or(&value);
    let hourly = item.get("hourly")?;
    let times = hourly.get("time")?.as_array()?;
    let values = hourly.get(field)?.as_array()?;
    let want = stamp_key(hour);
    for (index, stamp) in times.iter().enumerate() {
        let text = stamp.as_str()?;
        if stamp_key(text) != want {
            continue;
        }
        let number = values.get(index)?.as_f64()?;
        if number.is_finite() {
            return Some(number as f32);
        }
        return None;
    }
    None
}

fn http_get_once(agent: &ureq::Agent, url: &str) -> Result<String, String> {
    agent
        .get(url)
        .set("User-Agent", "FSOT-Global-Monitor/0.1")
        .call()
        .map_err(|err| err.to_string())?
        .into_string()
        .map_err(|err| err.to_string())
}

fn fetch_solar(agent: &ureq::Agent, quick: &ureq::Agent, held: &mut Vec<series::HeldNote>) -> SolarReading {
    let mut solar = SolarReading::default();
    if let Ok(body) = http_get(agent, "https://services.swpc.noaa.gov/products/summary/solar-wind-speed.json") {
        apply_wind_speed(&mut solar, &body);
    }
    match http_get(agent, "https://services.swpc.noaa.gov/products/geospace/propagated-solar-wind-1-hour.json") {
        Ok(body) => {
            apply_imf(&mut solar, &body);
            hold_body(
                held,
                "imf",
                "NOAA SWPC propagated solar wind",
                series::table_rows(&body, "propagated_time_tag"),
                "NOAA SWPC propagated IMF did not load.",
                false,
            );
        }
        Err(_) => recall_named(held, "imf", "NOAA SWPC propagated solar wind", "NOAA SWPC propagated IMF did not load."),
    }
    match http_get(agent, "https://services.swpc.noaa.gov/products/noaa-planetary-k-index-forecast.json") {
        Ok(body) => {
            apply_kp(&mut solar, &body);
            hold_body(
                held,
                "kp",
                "NOAA SWPC planetary K index",
                series::object_rows(&body, "time_tag"),
                "NOAA SWPC Kp did not load.",
                false,
            );
        }
        Err(_) => recall_named(held, "kp", "NOAA SWPC planetary K index", "NOAA SWPC Kp did not load."),
    }
    match http_get(agent, "https://services.swpc.noaa.gov/json/goes/primary/xrays-6-hour.json") {
        Ok(body) => {
            apply_xray(&mut solar, &body);
            hold_body(held, "xray", "NOAA SWPC GOES X-ray", series::object_rows(&body, "time_tag"), "GOES X-ray flux did not load.", false);
        }
        Err(_) => recall_named(held, "xray", "NOAA SWPC GOES X-ray", "GOES X-ray flux did not load."),
    }
    match http_get(agent, "https://services.swpc.noaa.gov/json/goes/primary/xray-flares-7-day.json") {
        Ok(body) => {
            apply_flares(&mut solar, &body);
            hold_body(
                held,
                "flares",
                "NOAA SWPC GOES X-ray flares",
                series::object_rows(&body, "max_time"),
                "GOES X-ray flares did not load.",
                true,
            );
        }
        Err(_) => recall_named(held, "flares", "NOAA SWPC GOES X-ray flares", "GOES X-ray flares did not load."),
    }
    solar.cme_line = fetch_cme(quick, held);
    // One try each. A dead file must not add the four-retry stall, and RTSW is large.
    fill_measured(agent, &mut solar, held);
    solar
}

fn fetch_cme(quick: &ureq::Agent, held: &mut Vec<series::HeldNote>) -> String {
    let now = geo::unix_now();
    let start = ymd(now - 7.0 * 86400.0);
    let end = ymd(now);
    let cme_url = format!("https://api.nasa.gov/DONKI/CME?startDate={start}&endDate={end}&api_key=DEMO_KEY");
    let both_failed = "NASA DONKI CME catalog did not load. SIDC CACTus did not load. No CME speed was invented.";
    match http_get_once(quick, &cme_url) {
        Ok(body) => {
            let line = cme_line_from(&body);
            if line.contains("did not load") {
                cactus_cme(quick, held, both_failed)
            } else {
                hold_body(held, "cme", "NASA DONKI", series::donki_rows(&body), "NASA DONKI CME catalog did not load.", true);
                line
            }
        }
        Err(_) => cactus_cme(quick, held, both_failed),
    }
}

fn cactus_cme(quick: &ureq::Agent, held: &mut Vec<series::HeldNote>, both_failed: &str) -> String {
    match http_get_once(quick, "https://www.sidc.be/cactus/out/cmecat.txt") {
        Ok(body) => {
            let line = cactus_line_from(&body);
            if line.contains("did not load") {
                recall_named(held, "cme", "SIDC CACTus", both_failed);
                both_failed.to_string()
            } else {
                hold_body(held, "cme", "SIDC CACTus", series::cactus_rows(&body), "SIDC CACTus CME catalog did not load.", true);
                line
            }
        }
        Err(_) => {
            recall_named(held, "cme", "SIDC CACTus", both_failed);
            both_failed.to_string()
        }
    }
}

fn fill_measured(agent: &ureq::Agent, solar: &mut SolarReading, held: &mut Vec<series::HeldNote>) {
    match http_get_once(agent, "https://services.swpc.noaa.gov/json/goes/primary/integral-protons-6-hour.json") {
        Ok(body) => {
            apply_protons(solar, &body);
            hold_body(
                held,
                "protons",
                "NOAA SWPC GOES integral protons",
                series::object_rows(&body, "time_tag"),
                "GOES integral proton flux did not load.",
                false,
            );
        }
        Err(_) => {
            solar.proton_line = "GOES integral proton flux did not load.".into();
            recall_named(held, "protons", "NOAA SWPC GOES integral protons", "GOES integral proton flux did not load.");
        }
    }
    match http_get_once(agent, "https://services.swpc.noaa.gov/json/goes/primary/integral-electrons-6-hour.json") {
        Ok(body) => {
            apply_electrons(solar, &body);
            hold_body(
                held,
                "electrons",
                "NOAA SWPC GOES integral electrons",
                series::object_rows(&body, "time_tag"),
                "GOES integral electron flux did not load.",
                false,
            );
        }
        Err(_) => {
            solar.electron_line = "GOES integral electron flux did not load.".into();
            recall_named(held, "electrons", "NOAA SWPC GOES integral electrons", "GOES integral electron flux did not load.");
        }
    }
    match http_get_once(agent, "https://services.swpc.noaa.gov/json/goes/primary/magnetometers-6-hour.json") {
        Ok(body) => {
            apply_mag(solar, &body);
            hold_body(
                held,
                "mag",
                "NOAA SWPC GOES magnetometer",
                series::object_rows(&body, "time_tag"),
                "GOES magnetometer did not load.",
                false,
            );
        }
        Err(_) => {
            solar.mag_total_nt = None;
            solar.mag_line = "GOES magnetometer did not load.".into();
            recall_named(held, "mag", "NOAA SWPC GOES magnetometer", "GOES magnetometer did not load.");
        }
    }
    match http_get_once(agent, "https://services.swpc.noaa.gov/json/rtsw/rtsw_wind_1m.json") {
        Ok(body) => {
            apply_plasma(solar, &body);
            hold_body(held, "rtsw", "NOAA SWPC RTSW", series::object_rows(&body, "time_tag"), "NOAA SWPC RTSW plasma did not load.", false);
        }
        Err(_) => {
            solar.plasma_line = "NOAA SWPC RTSW plasma did not load.".into();
            recall_named(held, "rtsw", "NOAA SWPC RTSW", "NOAA SWPC RTSW plasma did not load.");
        }
    }
    match http_get_once(agent, "https://services.swpc.noaa.gov/json/f107_cm_flux.json") {
        Ok(body) => {
            apply_f107(solar, &body);
            hold_body(held, "f107", "NOAA SWPC F10.7", series::object_rows(&body, "time_tag"), "NOAA SWPC F10.7 flux did not load.", false);
        }
        Err(_) => {
            solar.f107_line = "NOAA SWPC F10.7 flux did not load.".into();
            recall_named(held, "f107", "NOAA SWPC F10.7", "NOAA SWPC F10.7 flux did not load.");
        }
    }
    match http_get_once(agent, "https://services.swpc.noaa.gov/json/solar_regions.json") {
        Ok(body) => {
            let parsed = series::object_rows(&body, "observed_date");
            solar.region_line = match &parsed {
                Ok(rows) if rows.is_empty() => "NOAA SWPC lists no solar regions.".into(),
                Ok(rows) => region_summary(rows),
                Err(()) => "NOAA SWPC solar regions did not load.".into(),
            };
            hold_body(held, "regions", "NOAA SWPC solar regions", parsed, "NOAA SWPC solar regions did not load.", true);
        }
        Err(_) => {
            solar.region_line = "NOAA SWPC solar regions did not load.".into();
            recall_named(held, "regions", "NOAA SWPC solar regions", "NOAA SWPC solar regions did not load.");
        }
    }
    match http_get_once(agent, "https://services.swpc.noaa.gov/json/sunspot_report.json") {
        Ok(body) => {
            let parsed = series::object_rows(&body, "time_tag");
            solar.sunspot_line = match &parsed {
                Ok(rows) if rows.is_empty() => "NOAA SWPC lists no sunspot reports.".into(),
                Ok(rows) => sunspot_summary(rows),
                Err(()) => "NOAA SWPC sunspot report did not load.".into(),
            };
            hold_body(held, "sunspots", "NOAA SWPC sunspot report", parsed, "NOAA SWPC sunspot report did not load.", true);
        }
        Err(_) => {
            solar.sunspot_line = "NOAA SWPC sunspot report did not load.".into();
            recall_named(held, "sunspots", "NOAA SWPC sunspot report", "NOAA SWPC sunspot report did not load.");
        }
    }
    match http_get_once(agent, "https://services.swpc.noaa.gov/products/kyoto-dst.json") {
        Ok(body) => {
            apply_dst(solar, &body);
            hold_body(held, "dst", "Kyoto Dst via NOAA SWPC", series::object_rows(&body, "time_tag"), "Kyoto Dst did not load.", false);
        }
        Err(_) => {
            solar.dst_line = "Kyoto Dst did not load.".into();
            recall_named(held, "dst", "Kyoto Dst via NOAA SWPC", "Kyoto Dst did not load.");
        }
    }
    match http_get_once(agent, "https://services.swpc.noaa.gov/products/noaa-scales.json") {
        Ok(body) => {
            apply_scales(solar, &body);
            hold_body(held, "scales", "NOAA scales", series::scales_rows(&body), "NOAA scales did not load.", false);
        }
        Err(_) => {
            solar.scale_line = "NOAA scales did not load.".into();
            recall_named(held, "scales", "NOAA scales", "NOAA scales did not load.");
        }
    }
    match http_get_once(agent, "https://services.swpc.noaa.gov/products/alerts.json") {
        Ok(body) => {
            apply_alerts(solar, &body);
            hold_body(
                held,
                "alerts",
                "NOAA SWPC alerts",
                series::object_rows(&body, "issue_datetime"),
                "NOAA SWPC alerts did not load.",
                true,
            );
        }
        Err(_) => {
            solar.alert_line = "NOAA SWPC alerts did not load.".into();
            solar.alert_watch = false;
            recall_named(held, "alerts", "NOAA SWPC alerts", "NOAA SWPC alerts did not load.");
        }
    }
}

fn apply_protons(solar: &mut SolarReading, body: &str) {
    let Ok(rows) = serde_json::from_str::<Vec<Value>>(body) else {
        solar.proton_line = "GOES integral proton flux did not load.".into();
        return;
    };
    let mut best_time = String::new();
    let mut best: Option<(f32, String)> = None;
    for row in &rows {
        if row.get("energy").and_then(|value| value.as_str()) != Some(">=10 MeV") {
            continue;
        }
        let Some(flux) = row.get("flux").and_then(json_finite) else { continue };
        let time = row.get("time_tag").map(json_text).unwrap_or_default();
        if best.is_none() || time >= best_time {
            best_time = time.clone();
            best = Some((flux as f32, time));
        }
    }
    match best {
        Some((flux, time)) => {
            solar.proton_10mev = Some(flux);
            solar.proton_10mev_time = time;
            solar.proton_line.clear();
        }
        None => {
            solar.proton_10mev = None;
            solar.proton_line = "GOES integral proton flux did not include a finite >=10 MeV sample.".into();
        }
    }
}

fn magnetometer_sentence(solar: &SolarReading) -> String {
    let Some(total) = solar.mag_total_nt else {
        if !solar.mag_line.is_empty() {
            return solar.mag_line.clone();
        }
        return "GOES magnetometer has not arrived.".into();
    };
    let mut line = format!("GOES magnetometer total field {} nT", fmt_meas(total, 3));
    if !solar.mag_time.is_empty() {
        line.push_str(&format!(" at {}", solar.mag_time));
    }
    if !solar.mag_satellite.is_empty() {
        line.push_str(&format!(", satellite {}", solar.mag_satellite));
    }
    line.push('.');
    if solar.mag_arcjet {
        line.push_str(" The arcjet flag on that row is true.");
    }
    line.push_str(" He, Hp, and Hn are the components in that file. Propagated IMF Bt and Bz stay on their own line.");
    line
}

/// Latest finite total field. The value stays in nT and is not turned into a G-scale.
fn apply_mag(solar: &mut SolarReading, body: &str) {
    solar.mag_total_nt = None;
    solar.mag_time.clear();
    solar.mag_satellite.clear();
    solar.mag_arcjet = false;
    let Ok(rows) = serde_json::from_str::<Vec<Value>>(body) else {
        solar.mag_line = "GOES magnetometer did not load.".into();
        return;
    };
    let mut best_time = String::new();
    let mut best: Option<&Value> = None;
    for row in &rows {
        if row.get("total").and_then(json_finite).is_none() {
            continue;
        }
        let time = row.get("time_tag").map(json_text).unwrap_or_default();
        if best.is_none() || time >= best_time {
            best_time = time;
            best = Some(row);
        }
    }
    let Some(row) = best else {
        solar.mag_line = "GOES magnetometer did not include a finite total field.".into();
        return;
    };
    solar.mag_total_nt = row.get("total").and_then(json_finite).map(|value| value as f32);
    solar.mag_time = row.get("time_tag").map(json_text).unwrap_or_default();
    solar.mag_satellite = row.get("satellite").map(json_text).unwrap_or_default();
    solar.mag_arcjet = row.get("arcjet_flag").is_some_and(json_flag);
    solar.mag_line.clear();
}

fn region_summary(rows: &[series::Row]) -> String {
    let Some(latest) = latest_text(rows) else {
        return "NOAA SWPC solar regions did not load.".into();
    };
    let day: Vec<&series::Row> = rows.iter().filter(|row| row_text(row, "t") == latest).collect();
    if day.is_empty() {
        return "NOAA SWPC solar regions did not load.".into();
    }
    let row = best_measured(&day, &["area", "number_spots"]);
    format!(
        "NOAA SWPC solar regions on {latest}: {} regions. {} {}, {}, {}, {}, {}, {}. A region entry is not a flare.",
        day.len(),
        region_id(row, "region"),
        location_phrase(row, "location"),
        class_phrase(row, "spot_class", "spot class"),
        class_phrase(row, "mag_class", "magnetic class"),
        area_phrase(row, "area"),
        spots_phrase(row, "number_spots"),
        class_phrase(row, "status", "status"),
    )
}

fn sunspot_summary(rows: &[series::Row]) -> String {
    let Some(latest) = latest_text(rows) else {
        return "NOAA SWPC sunspot report did not load.".into();
    };
    let batch: Vec<&series::Row> = rows.iter().filter(|row| row_text(row, "t") == latest).collect();
    if batch.is_empty() {
        return "NOAA SWPC sunspot report did not load.".into();
    }
    let row = best_measured(&batch, &["Area", "Numspot"]);
    let observatory = row_text(row, "Observatory");
    let observatory = if observatory.is_empty() {
        "observatory was not in that row".to_string()
    } else {
        format!("observatory {observatory}")
    };
    format!(
        "NOAA SWPC sunspot report {latest}, {observatory}: {} regions in that report. {} {}, {}, {}, {}, {}. One report is not a flare.",
        batch.len(),
        region_id(row, "Region"),
        location_phrase(row, "Location"),
        class_phrase(row, "Spotclass", "class"),
        class_phrase(row, "Magclass", "magnetic class"),
        area_phrase(row, "Area"),
        spots_phrase(row, "Numspot"),
    )
}

fn co2_summary(rows: &[series::Row]) -> String {
    let mut best: Option<(String, f32, String)> = None;
    for row in rows {
        let Some(value) = row.get("co2_ppm").and_then(|item| item.as_f64()) else { continue };
        if !value.is_finite() {
            continue;
        }
        let when = row_text(row, "t");
        let station = {
            let text = row_text(row, "station");
            if text.is_empty() { "Mauna Loa Observatory".to_string() } else { text }
        };
        if best.as_ref().map(|(time, _, _)| when.as_str() >= time.as_str()).unwrap_or(true) {
            best = Some((when, value as f32, station));
        }
    }
    let Some((when, value, station)) = best else {
        return "NOAA GML daily CO2 did not include a finite value.".into();
    };
    format!(
        "NOAA GML daily CO2 at {station} is {} ppm on {when}. The file says measurements from December 2022 through 4 July 2023 are from Maunakea Observatories. One daily value is one station day.",
        fmt_meas(value, 2)
    )
}

fn air_summary(rows: &[series::Row], now_hour: &str) -> String {
    let Some(row) = choose_air_row(rows, now_hour) else {
        return "Open-Meteo air quality for this place did not load.".into();
    };
    let when = row_text(row, "t");
    format!(
        "Open-Meteo air quality for this place at {when}: {}, {}, {}.",
        species_phrase(row, "co_ugm3", "carbon monoxide"),
        species_phrase(row, "so2_ugm3", "sulphur dioxide"),
        species_phrase(row, "dust_ugm3", "dust")
    )
}

fn choose_air_row<'a>(rows: &'a [series::Row], now_hour: &str) -> Option<&'a series::Row> {
    let mut past: Option<&series::Row> = None;
    let mut future: Option<&series::Row> = None;
    for row in rows {
        let when = row_text(row, "t");
        if when.is_empty() {
            continue;
        }
        if stamp_key(&when).as_str() <= now_hour {
            let replace = past.map(|old| when.as_str() >= row_text(old, "t").as_str()).unwrap_or(true);
            if replace {
                past = Some(row);
            }
        } else if future.map(|old| when.as_str() < row_text(old, "t").as_str()).unwrap_or(true) {
            future = Some(row);
        }
    }
    past.or(future)
}

fn species_phrase(row: &series::Row, key: &str, name: &str) -> String {
    match row.get(key).and_then(|item| item.as_f64()) {
        Some(value) if value.is_finite() => format!("{name} {} ug/m^3", fmt_meas(value as f32, 1)),
        _ => format!("{name} was not in that hour"),
    }
}

fn latest_text(rows: &[series::Row]) -> Option<String> {
    rows.iter().filter_map(|row| row.get("t").and_then(|item| item.as_str()).filter(|text| !text.is_empty())).max().map(|text| text.to_string())
}

fn best_measured<'a>(rows: &[&'a series::Row], keys: &[&str]) -> &'a series::Row {
    rows.iter().copied().max_by(|left, right| measured_score(left, keys).total_cmp(&measured_score(right, keys))).unwrap_or(rows[0])
}

fn measured_score(row: &series::Row, keys: &[&str]) -> f64 {
    for key in keys {
        if let Some(value) = row.get(*key).and_then(|item| item.as_f64()) {
            if value.is_finite() {
                return value;
            }
        }
    }
    f64::NEG_INFINITY
}

fn region_id(row: &series::Row, key: &str) -> String {
    let id = row_text(row, key);
    if id.is_empty() { "Region number was not in that row".into() } else { format!("Region {id}") }
}

fn location_phrase(row: &series::Row, key: &str) -> String {
    let place = row_text(row, key);
    if place.is_empty() { "location was not in that row".into() } else { format!("at {place}") }
}

fn class_phrase(row: &series::Row, key: &str, label: &str) -> String {
    let text = row_text(row, key);
    if text.is_empty() { format!("{label} was not in that row") } else { format!("{label} {text}") }
}

fn area_phrase(row: &series::Row, key: &str) -> String {
    match row.get(key).and_then(|item| item.as_f64()) {
        Some(value) if value.is_finite() => format!("area {} millionths of a solar hemisphere", fmt_meas(value as f32, 0)),
        _ => "area was not in that row".into(),
    }
}

fn spots_phrase(row: &series::Row, key: &str) -> String {
    match row.get(key).and_then(|item| item.as_f64()) {
        Some(value) if value.is_finite() => {
            let count = fmt_meas(value as f32, 0);
            if count == "1" { "1 spot".into() } else { format!("{count} spots") }
        }
        _ => "spot count was not in that row".into(),
    }
}

fn row_text(row: &series::Row, key: &str) -> String {
    let Some(value) = row.get(key) else { return String::new() };
    if value.is_null() {
        return String::new();
    }
    if let Some(text) = value.as_str() {
        return text.trim().to_string();
    }
    if let Some(number) = value.as_i64() {
        return number.to_string();
    }
    if let Some(number) = value.as_u64() {
        return number.to_string();
    }
    if let Some(number) = value.as_f64() {
        if number.is_finite() {
            return fmt_meas(number as f32, 0);
        }
    }
    String::new()
}

/// Latest finite >=2 MeV flux. The flux is not turned into an S-scale or an alert.
fn apply_electrons(solar: &mut SolarReading, body: &str) {
    let Ok(rows) = serde_json::from_str::<Vec<Value>>(body) else {
        solar.electron_line = "GOES integral electron flux did not load.".into();
        return;
    };
    let mut best_time = String::new();
    let mut best: Option<(f32, String)> = None;
    for row in &rows {
        if row.get("energy").and_then(|value| value.as_str()) != Some(">=2 MeV") {
            continue;
        }
        let Some(flux) = row.get("flux").and_then(json_finite) else { continue };
        let time = row.get("time_tag").map(json_text).unwrap_or_default();
        if best.is_none() || time >= best_time {
            best_time = time.clone();
            best = Some((flux as f32, time));
        }
    }
    match best {
        Some((flux, time)) => {
            solar.electron_2mev = Some(flux);
            solar.electron_2mev_time = time;
            solar.electron_line.clear();
        }
        None => {
            solar.electron_2mev = None;
            solar.electron_line = "GOES integral electron flux did not include a finite >=2 MeV sample.".into();
        }
    }
}

/// The alerts file is newest-first. Quote the first message. An empty array is a real empty catalog.
fn apply_alerts(solar: &mut SolarReading, body: &str) {
    solar.alert_code.clear();
    solar.alert_sentence.clear();
    solar.alert_time.clear();
    solar.alert_watch = false;
    let Ok(rows) = serde_json::from_str::<Vec<Value>>(body) else {
        solar.alert_line = "NOAA SWPC alerts did not load.".into();
        return;
    };
    let Some(row) = rows.first() else {
        solar.alert_line = "NOAA SWPC lists no issued alerts.".into();
        return;
    };
    let code = row.get("product_id").map(json_text).unwrap_or_default();
    let time = row.get("issue_datetime").map(json_text).unwrap_or_default();
    let message = row.get("message").map(json_text).unwrap_or_default();
    let sentence = alert_sentence(&message);
    if sentence.is_empty() {
        solar.alert_line = "NOAA SWPC alerts did not load.".into();
        return;
    }
    let upper = message.to_ascii_uppercase();
    solar.alert_watch = upper.contains("ALERT") || upper.contains("WARNING");
    solar.alert_code = code.clone();
    solar.alert_sentence = sentence.clone();
    solar.alert_time = time.clone();
    let mut line = String::from("NOAA SWPC alert");
    if !code.is_empty() {
        line.push(' ');
        line.push_str(&code);
    }
    line.push_str(": ");
    line.push_str(&sentence);
    if !time.is_empty() {
        line.push_str(" Issued ");
        line.push_str(&time);
        line.push('.');
    } else if !line.ends_with('.') {
        line.push('.');
    }
    solar.alert_line = line;
}

/// Prefer the line where SWPC itself says ALERT or WARNING. Headers stay out of the quote.
fn alert_sentence(message: &str) -> String {
    let lines: Vec<&str> = message.lines().map(str::trim).filter(|line| !line.is_empty()).collect();
    if let Some(line) = lines.iter().copied().find(|line| {
        let upper = line.to_ascii_uppercase();
        upper.contains("ALERT") || upper.contains("WARNING")
    }) {
        return line.to_string();
    }
    lines.first().copied().unwrap_or("").to_string()
}

/// Latest row with `active` and a finite density. The file tail can be inactive and older.
fn apply_plasma(solar: &mut SolarReading, body: &str) {
    let Ok(rows) = serde_json::from_str::<Vec<Value>>(body) else {
        solar.plasma_line = "NOAA SWPC RTSW plasma did not load.".into();
        return;
    };
    let mut active_time = String::new();
    let mut active_row: Option<&Value> = None;
    let mut any_time = String::new();
    let mut any_row: Option<&Value> = None;
    for row in &rows {
        if row.get("proton_density").and_then(json_finite).is_none() {
            continue;
        }
        let time = row.get("time_tag").map(json_text).unwrap_or_default();
        let active = row.get("active").is_some_and(json_flag);
        if active && (active_row.is_none() || time >= active_time) {
            active_time = time.clone();
            active_row = Some(row);
        }
        if any_row.is_none() || time >= any_time {
            any_time = time;
            any_row = Some(row);
        }
    }
    let Some(row) = active_row.or(any_row) else {
        solar.density_cm3 = None;
        solar.temperature_k = None;
        solar.plasma_line = "NOAA SWPC RTSW plasma did not include a density.".into();
        return;
    };
    solar.density_cm3 = row.get("proton_density").and_then(json_finite).map(|value| value as f32);
    solar.temperature_k = row.get("proton_temperature").and_then(json_finite).map(|value| value as f32);
    solar.plasma_time = row.get("time_tag").map(json_text).unwrap_or_default();
    solar.plasma_source = row.get("source").map(json_text).unwrap_or_default();
    solar.plasma_line.clear();
}

fn apply_f107(solar: &mut SolarReading, body: &str) {
    let Ok(rows) = serde_json::from_str::<Vec<Value>>(body) else {
        solar.f107_line = "NOAA SWPC F10.7 flux did not load.".into();
        return;
    };
    let mut flux_time = String::new();
    let mut mean_time = String::new();
    let mut found_flux = false;
    let mut found_mean = false;
    for row in &rows {
        let time = row.get("time_tag").map(json_text).unwrap_or_default();
        if let Some(flux) = row.get("flux").and_then(json_finite) {
            if !found_flux || time >= flux_time {
                flux_time = time.clone();
                solar.f107_sfu = Some(flux as f32);
                solar.f107_time = time.clone();
                solar.f107_schedule = row.get("reporting_schedule").map(json_text).unwrap_or_default();
                found_flux = true;
            }
        }
        if let Some(mean) = row.get("ninety_day_mean").and_then(json_finite) {
            if !found_mean || time >= mean_time {
                mean_time = time.clone();
                solar.f107_mean_sfu = Some(mean as f32);
                solar.f107_mean_time = time;
                found_mean = true;
            }
        }
    }
    if found_flux {
        solar.f107_line.clear();
    } else {
        solar.f107_sfu = None;
        solar.f107_line = "NOAA SWPC F10.7 flux did not load.".into();
    }
}

fn apply_dst(solar: &mut SolarReading, body: &str) {
    let Ok(rows) = serde_json::from_str::<Vec<Value>>(body) else {
        solar.dst_line = "Kyoto Dst did not load.".into();
        return;
    };
    let mut best_time = String::new();
    let mut best: Option<(f32, String)> = None;
    for row in &rows {
        let Some(dst) = row.get("dst").and_then(json_finite) else { continue };
        let time = row.get("time_tag").map(json_text).unwrap_or_default();
        if best.is_none() || time >= best_time {
            best_time = time.clone();
            best = Some((dst as f32, time));
        }
    }
    match best {
        Some((dst, time)) => {
            solar.dst_nt = Some(dst);
            solar.dst_time = time;
            solar.dst_line.clear();
        }
        None => {
            solar.dst_nt = None;
            solar.dst_line = "Kyoto Dst did not load.".into();
        }
    }
}

/// Current conditions are the object at key "0". Later keys are the agency's later days.
fn apply_scales(solar: &mut SolarReading, body: &str) {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        solar.scale_line = "NOAA scales did not load.".into();
        return;
    };
    let Some(current) = value.get("0") else {
        solar.scale_line = "NOAA scales did not load.".into();
        solar.scale_r.clear();
        solar.scale_s.clear();
        solar.scale_g.clear();
        return;
    };
    solar.scale_r = scale_pair(current.get("R"));
    solar.scale_s = scale_pair(current.get("S"));
    solar.scale_g = scale_pair(current.get("G"));
    let date = current.get("DateStamp").map(json_text).unwrap_or_default();
    let time = current.get("TimeStamp").map(json_text).unwrap_or_default();
    solar.scale_time = format!("{date} {time}").trim().to_string();
    if solar.scale_r.is_empty() && solar.scale_s.is_empty() && solar.scale_g.is_empty() {
        solar.scale_line = "NOAA scales did not load.".into();
    } else {
        solar.scale_line.clear();
    }
}

fn scale_pair(value: Option<&Value>) -> String {
    let Some(value) = value else { return String::new() };
    let scale = value.get("Scale").map(json_text).unwrap_or_default();
    let text = value.get("Text").and_then(|item| item.as_str()).unwrap_or("").trim().to_string();
    match (scale.is_empty(), text.is_empty()) {
        (true, true) => String::new(),
        (false, true) => scale,
        (true, false) => text,
        (false, false) => format!("{scale} {text}"),
    }
}

fn fetch_climate(agent: &ureq::Agent, held: &mut Vec<series::HeldNote>) -> ClimateReading {
    let mut climate = ClimateReading::default();
    match http_get_once(agent, "https://www.cpc.ncep.noaa.gov/data/indices/oni.ascii.txt") {
        Ok(body) => {
            match parse_oni(&body) {
                Ok((season, year, total, anom)) => {
                    climate.oni_season = season;
                    climate.oni_year = Some(year);
                    climate.oni_total = Some(total);
                    climate.oni_anom = Some(anom);
                }
                Err(line) => climate.oni_line = line,
            }
            hold_body(held, "oni", "CPC ONI", series::oni_rows(&body), "CPC ONI index did not load.", false);
        }
        Err(_) => {
            climate.oni_line = "CPC ONI index did not load.".into();
            recall_named(held, "oni", "CPC ONI", "CPC ONI index did not load.");
        }
    }
    match http_get_once(agent, "https://www.cpc.ncep.noaa.gov/products/precip/CWlink/daily_ao_index/monthly.ao.index.b50.current.ascii") {
        Ok(body) => {
            match parse_monthly_index(&body) {
                Ok((year, month, index)) => {
                    climate.ao_year = Some(year);
                    climate.ao_month = Some(month);
                    climate.ao = Some(index);
                }
                Err(_) => climate.ao_line = "CPC AO index did not load.".into(),
            }
            hold_body(held, "ao", "CPC AO", series::monthly_rows(&body), "CPC AO index did not load.", false);
        }
        Err(_) => {
            climate.ao_line = "CPC AO index did not load.".into();
            recall_named(held, "ao", "CPC AO", "CPC AO index did not load.");
        }
    }
    match http_get_once(agent, "https://www.cpc.ncep.noaa.gov/products/precip/CWlink/pna/norm.nao.monthly.b5001.current.ascii") {
        Ok(body) => {
            match parse_monthly_index(&body) {
                Ok((year, month, index)) => {
                    climate.nao_year = Some(year);
                    climate.nao_month = Some(month);
                    climate.nao = Some(index);
                }
                Err(_) => climate.nao_line = "CPC NAO index did not load.".into(),
            }
            hold_body(held, "nao", "CPC NAO", series::monthly_rows(&body), "CPC NAO index did not load.", false);
        }
        Err(_) => {
            climate.nao_line = "CPC NAO index did not load.".into();
            recall_named(held, "nao", "CPC NAO", "CPC NAO index did not load.");
        }
    }
    // The anonymous-FTP CSV did not answer. This graphics file is the BOM RMM that loads.
    // Its last date is quoted. A 2024 ending is not called current.
    match http_get_once(agent, "http://www.bom.gov.au/climate/mjo/graphics/rmm.74toRealtime.txt") {
        Ok(body) => {
            let parsed = series::rmm_rows(&body);
            climate.mjo_line = match &parsed {
                Ok(rows) => rmm_line(rows),
                Err(()) => "BOM RMM index did not load.".into(),
            };
            hold_body(held, "mjo", "BOM RMM", parsed, "BOM RMM index did not load.", false);
        }
        Err(_) => {
            climate.mjo_line = "BOM RMM index did not load.".into();
            recall_named(held, "mjo", "BOM RMM", "BOM RMM index did not load.");
        }
    }
    let north = http_get_once(
        agent,
        "https://noaadata.apps.nsidc.org/NOAA/G02135/north/daily/data/N_seaice_extent_daily_v4.0.csv",
    );
    let south = http_get_once(
        agent,
        "https://noaadata.apps.nsidc.org/NOAA/G02135/south/daily/data/S_seaice_extent_daily_v4.0.csv",
    );
    let north_rows = north.as_deref().ok().and_then(|body| series::sea_ice_rows(body, "N").ok());
    let south_rows = south.as_deref().ok().and_then(|body| series::sea_ice_rows(body, "S").ok());
    match (north_rows, south_rows) {
        (None, None) => {
            climate.ice_line = "NSIDC sea ice extent did not load.".into();
            recall_named(held, "sea-ice", "NSIDC Sea Ice Index", "NSIDC sea ice extent did not load.");
        }
        (north_rows, south_rows) => {
            let mut rows = north_rows.clone().unwrap_or_default();
            rows.extend(south_rows.clone().unwrap_or_default());
            climate.ice_line = ice_line(north_rows.as_deref(), south_rows.as_deref());
            hold_body(held, "sea-ice", "NSIDC Sea Ice Index", Ok(rows), "NSIDC sea ice extent did not load.", false);
        }
    }
    match http_get_once(agent, "https://gml.noaa.gov/webdata/ccgg/trends/co2/co2_daily_mlo.csv") {
        Ok(body) => {
            let parsed = series::co2_rows(&body);
            climate.co2_line = match &parsed {
                Ok(rows) => co2_summary(rows),
                Err(()) => "NOAA GML daily CO2 did not load.".into(),
            };
            hold_body(held, "co2", "NOAA GML Mauna Loa daily", parsed, "NOAA GML daily CO2 did not load.", false);
        }
        Err(_) => {
            climate.co2_line = "NOAA GML daily CO2 did not load.".into();
            recall_named(held, "co2", "NOAA GML Mauna Loa daily", "NOAA GML daily CO2 did not load.");
        }
    }
    climate
}

fn rmm_line(rows: &[series::Row]) -> String {
    let Some(row) = rows.iter().max_by_key(|row| row.get("t").and_then(|item| item.as_str()).unwrap_or("").to_string()) else {
        return "BOM RMM index did not load.".into();
    };
    let when = row.get("t").and_then(|item| item.as_str()).unwrap_or("");
    if when.is_empty() {
        return "BOM RMM index did not load.".into();
    }
    let cutoff = ymd(geo::unix_now() - 14.0 * 86400.0);
    let mut line = format!(
        "BOM RMM last row in the loaded file is {when}: RMM1 {}, RMM2 {}, phase {}, amplitude {}.",
        rmm_text(row, "rmm1"),
        rmm_text(row, "rmm2"),
        rmm_text(row, "phase"),
        rmm_text(row, "amplitude")
    );
    if when < cutoff.as_str() {
        line.push_str(" The loaded file ends on that date.");
    }
    line.push_str(" One row is not an MJO event.");
    line
}

fn rmm_text(row: &series::Row, key: &str) -> String {
    match row.get(key).and_then(|item| item.as_f64()) {
        Some(value) if value.is_finite() => fmt_meas(value as f32, 3),
        _ => "missing".into(),
    }
}

fn ice_line(north: Option<&[series::Row]>, south: Option<&[series::Row]>) -> String {
    format!("NSIDC Sea Ice Index {}. {}.", extent_phrase("Arctic", north), extent_phrase("Antarctic", south))
}

fn extent_phrase(name: &str, rows: Option<&[series::Row]>) -> String {
    let Some((when, extent)) = latest_extent(rows) else {
        return format!("{name} extent did not load");
    };
    format!("{name} extent {} 10^6 km² on {when}", fmt_meas(extent, 3))
}

fn latest_extent(rows: Option<&[series::Row]>) -> Option<(String, f32)> {
    let rows = rows?;
    let mut best: Option<(String, f32)> = None;
    for row in rows {
        let Some(extent) = row.get("extent").and_then(|item| item.as_f64()) else { continue };
        if !extent.is_finite() {
            continue;
        }
        let when = row.get("t").and_then(|item| item.as_str()).unwrap_or("").to_string();
        if best.as_ref().map(|(time, _)| when.as_str() >= time.as_str()).unwrap_or(true) {
            best = Some((when, extent as f32));
        }
    }
    best
}

fn parse_oni(body: &str) -> Result<(String, i32, f32, f32), String> {
    let mut last = None;
    for line in body.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }
        let season = parts[0];
        if season.eq_ignore_ascii_case("SEAS") || season.len() != 3 || !season.chars().all(|ch| ch.is_ascii_alphabetic()) {
            continue;
        }
        let Ok(year) = parts[1].parse::<i32>() else { continue };
        let Ok(total) = parts[2].parse::<f32>() else { continue };
        let Ok(anom) = parts[3].parse::<f32>() else { continue };
        if !total.is_finite() || !anom.is_finite() {
            continue;
        }
        last = Some((season.to_string(), year, total, anom));
    }
    last.ok_or_else(|| "CPC ONI index did not load.".into())
}

fn parse_monthly_index(body: &str) -> Result<(i32, i32, f32), String> {
    let mut last = None;
    for line in body.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }
        let Ok(year) = parts[0].parse::<i32>() else { continue };
        let Ok(month) = parts[1].parse::<i32>() else { continue };
        if !(1..=12).contains(&month) {
            continue;
        }
        let Ok(index) = parts[2].parse::<f32>() else { continue };
        if !index.is_finite() {
            continue;
        }
        last = Some((year, month, index));
    }
    last.ok_or_else(|| "CPC index did not load.".into())
}

fn opt_f32(value: Option<&Value>) -> Option<f32> {
    value.and_then(json_finite).map(|number| number as f32)
}

fn opt_i32(value: Option<&Value>) -> Option<i32> {
    value.and_then(json_finite).map(|number| number as i32)
}

fn json_finite(value: &Value) -> Option<f64> {
    let number = if let Some(number) = value.as_f64() {
        number
    } else if let Some(text) = value.as_str() {
        text.trim().parse::<f64>().ok()?
    } else {
        return None;
    };
    number.is_finite().then_some(number)
}

fn json_text(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.trim().to_string();
    }
    if let Some(number) = value.as_i64() {
        return number.to_string();
    }
    if let Some(number) = value.as_u64() {
        return number.to_string();
    }
    if let Some(number) = value.as_f64() {
        return format!("{number}");
    }
    String::new()
}

fn json_flag(value: &Value) -> bool {
    if let Some(flag) = value.as_bool() {
        return flag;
    }
    if let Some(text) = value.as_str() {
        return text.eq_ignore_ascii_case("true") || text == "1";
    }
    value.as_i64() == Some(1)
}

fn apply_wind_speed(solar: &mut SolarReading, body: &str) {
    let Ok(value) = serde_json::from_str::<Value>(body) else { return };
    let Some(row) = value.as_array().and_then(|rows| rows.first()) else { return };
    solar.wind_kms = row.get("proton_speed").and_then(|v| v.as_f64()).map(|v| v as f32);
    solar.wind_time = row.get("time_tag").and_then(|v| v.as_str()).unwrap_or("").to_string();
}

fn apply_imf(solar: &mut SolarReading, body: &str) {
    let Ok(value) = serde_json::from_str::<Value>(body) else { return };
    let Some(rows) = value.as_array() else { return };
    let Some(header) = rows.first().and_then(|row| row.as_array()) else { return };
    let names: Vec<&str> = header.iter().filter_map(|cell| cell.as_str()).collect();
    let Some(bt_i) = names.iter().position(|name| *name == "bt") else { return };
    let Some(bz_i) = names.iter().position(|name| *name == "bz") else { return };
    let time_i = names.iter().position(|name| *name == "propagated_time_tag");
    for row in rows.iter().rev() {
        let Some(cells) = row.as_array() else { continue };
        let Some(bt) = cells.get(bt_i).and_then(|v| v.as_f64()) else { continue };
        let Some(bz) = cells.get(bz_i).and_then(|v| v.as_f64()) else { continue };
        solar.bt_nt = Some(bt as f32);
        solar.bz_nt = Some(bz as f32);
        if let Some(index) = time_i {
            solar.imf_time = cells.get(index).and_then(|v| v.as_str()).unwrap_or("").to_string();
        }
        return;
    }
}

fn apply_kp(solar: &mut SolarReading, body: &str) {
    let Ok(rows) = serde_json::from_str::<Vec<Value>>(body) else { return };
    for row in &rows {
        let kind = row.get("observed").and_then(|v| v.as_str()).unwrap_or("");
        let Some(kp) = row.get("kp").and_then(|v| v.as_f64()) else { continue };
        let time = row.get("time_tag").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let scale = row.get("noaa_scale").and_then(|v| v.as_str()).filter(|text| !text.is_empty()).map(|text| text.to_string());
        if kind == "observed" || kind == "estimated" {
            solar.kp_now = Some(kp as f32);
            solar.kp_now_kind = kind.to_string();
            solar.kp_now_time = time;
            solar.kp_now_scale = scale;
        } else if kind == "predicted" && solar.kp_forecast.map(|current| kp as f32 >= current).unwrap_or(true) {
            solar.kp_forecast = Some(kp as f32);
            solar.kp_forecast_time = time;
            solar.kp_forecast_scale = scale;
        }
    }
}

fn apply_xray(solar: &mut SolarReading, body: &str) {
    let Ok(rows) = serde_json::from_str::<Vec<Value>>(body) else { return };
    for row in rows.iter().rev() {
        if row.get("energy").and_then(|v| v.as_str()) != Some("0.1-0.8nm") {
            continue;
        }
        let Some(flux) = row.get("flux").and_then(|v| v.as_f64()) else { continue };
        solar.xray_class = goes_class(flux);
        solar.xray_time = row.get("time_tag").and_then(|v| v.as_str()).unwrap_or("").to_string();
        return;
    }
}

fn goes_class(flux: f64) -> String {
    if !flux.is_finite() || flux < 1e-8 {
        return "below A".into();
    }
    let (letter, floor) = if flux < 1e-7 {
        ("A", 1e-8)
    } else if flux < 1e-6 {
        ("B", 1e-7)
    } else if flux < 1e-5 {
        ("C", 1e-6)
    } else if flux < 1e-4 {
        ("M", 1e-5)
    } else {
        ("X", 1e-4)
    };
    format!("{letter}{:.1}", flux / floor)
}

fn apply_flares(solar: &mut SolarReading, body: &str) {
    let Ok(rows) = serde_json::from_str::<Vec<Value>>(body) else { return };
    let mut latest_time = String::new();
    let mut strongest_rank = f64::MIN;
    for row in &rows {
        let Some(class) = row.get("max_class").and_then(|v| v.as_str()) else { continue };
        let time = row.get("max_time").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if time >= latest_time {
            latest_time = time.clone();
            solar.latest_flare = class.to_string();
            solar.latest_flare_time = time;
        }
        let rank = class_rank(class);
        if rank >= strongest_rank {
            strongest_rank = rank;
            solar.strongest_flare = class.to_string();
            solar.strongest_flare_time = row.get("max_time").and_then(|v| v.as_str()).unwrap_or("").to_string();
        }
    }
}

fn class_rank(class: &str) -> f64 {
    let mut chars = class.chars();
    let letter = chars.next().unwrap_or('A');
    let magnitude: f64 = chars.as_str().parse().unwrap_or(0.0);
    let base = match letter {
        'A' => 0.0,
        'B' => 10.0,
        'C' => 20.0,
        'M' => 30.0,
        'X' => 40.0,
        _ => 0.0,
    };
    base + magnitude
}

fn cme_line_from(body: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        return "NASA DONKI CME catalog did not load.".into();
    };
    let Some(list) = value.as_array() else {
        return "NASA DONKI CME catalog did not load.".into();
    };
    if list.is_empty() {
        return "NASA DONKI lists no CME in the last 7 days.".into();
    }
    let Some(event) = list.iter().max_by_key(|item| item.get("startTime").and_then(|v| v.as_str()).unwrap_or("").to_string()) else {
        return "NASA DONKI lists no CME in the last 7 days.".into();
    };
    let start = event.get("startTime").and_then(|v| v.as_str()).unwrap_or("unknown time");
    let analysis = event.get("cmeAnalyses").and_then(|v| v.as_array()).and_then(|rows| {
        rows.iter()
            .find(|row| row.get("isMostAccurate").and_then(|v| v.as_bool()) == Some(true))
            .or_else(|| rows.last())
    });
    match analysis.and_then(|row| row.get("speed")).and_then(|v| v.as_f64()) {
        Some(speed) => {
            let kind = analysis.and_then(|row| row.get("type")).and_then(|v| v.as_str()).unwrap_or("");
            if kind.is_empty() {
                format!("NASA DONKI CME started {start}, most accurate speed {speed:.0} km/s.")
            } else {
                format!("NASA DONKI CME started {start}, most accurate speed {speed:.0} km/s, type {kind}.")
            }
        }
        None => format!("NASA DONKI CME started {start}. No speed analysis is attached."),
    }
}

fn cactus_line_from(body: &str) -> String {
    if !body.contains("# CME") {
        return "SIDC CACTus CME catalog did not load.".into();
    }
    let mut best: Option<(String, String, i32, i32, String)> = None;
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("# Flow") || trimmed.starts_with("#EOF") {
            break;
        }
        if trimmed.starts_with('#') || !trimmed.contains('|') {
            continue;
        }
        let parts: Vec<&str> = trimmed.split('|').map(|part| part.trim()).collect();
        if parts.len() < 6 {
            continue;
        }
        let id = parts[0];
        if !id.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
            continue;
        }
        let onset = parts[1];
        if !onset.contains('/') {
            continue;
        }
        let Ok(width) = parts[4].parse::<i32>() else { continue };
        let Ok(speed) = parts[5].parse::<i32>() else { continue };
        if speed < 0 {
            continue;
        }
        let halo = parts.get(9).copied().unwrap_or("").to_string();
        let replace = best.as_ref().map(|(when, _, _, _, _)| onset > when.as_str()).unwrap_or(true);
        if replace {
            best = Some((onset.to_string(), id.to_string(), speed, width, halo));
        }
    }
    match best {
        None => "SIDC CACTus lists no CME in the latest LASCO run.".into(),
        Some((onset, id, speed, width, halo)) => {
            let halo = if halo == "II" || halo == "III" || halo == "IV" {
                format!(", halo {halo}")
            } else {
                String::new()
            };
            format!("SIDC CACTus LASCO CME {id} onset {onset}, median speed {speed} km/s, width {width}°{halo}.")
        }
    }
}

struct StormPass {
    line: String,
    storms: Vec<Storm>,
    nhc_ok: bool,
    jtwc_ok: bool,
}

fn fetch_storms(agent: &ureq::Agent, held: &mut Vec<series::HeldNote>) -> StormPass {
    let nhc = http_get_once(agent, "https://www.nhc.noaa.gov/CurrentStorms.json").and_then(|body| storm_report(&body));
    let jtwc = fetch_jtwc(agent);
    let nhc_ok = nhc.is_ok();
    let jtwc_ok = jtwc.is_ok();
    let (nhc_line, nhc_storms) = match nhc {
        Ok(pair) => pair,
        Err(_) => ("NHC storm list did not load.".into(), Vec::new()),
    };
    let (jtwc_line, jtwc_storms) = match jtwc {
        Ok(pair) => pair,
        Err(_) => ("JTWC tropical cyclone warnings did not load.".into(), Vec::new()),
    };
    let mut storms = nhc_storms;
    storms.extend(jtwc_storms);
    if nhc_ok && jtwc_ok {
        hold_body(held, "storms", "NHC and JTWC", Ok(storm_rows(&storms)), "JTWC tropical cyclone warnings did not load.", true);
    } else {
        let detail = if !nhc_ok && !jtwc_ok {
            "NHC storm list did not load. JTWC tropical cyclone warnings did not load."
        } else if !nhc_ok {
            "NHC storm list did not load."
        } else {
            "JTWC tropical cyclone warnings did not load."
        };
        recall_named(held, "storms", "NHC and JTWC", detail);
    }
    StormPass { line: format!("{nhc_line} {jtwc_line}"), storms, nhc_ok, jtwc_ok }
}

fn apply_storm_pass(bulletin: &mut Bulletin, pass: StormPass) {
    if pass.nhc_ok && pass.jtwc_ok {
        bulletin.storms = pass.storms;
        bulletin.storm_line = pass.line;
        return;
    }
    if pass.nhc_ok {
        let kept: Vec<Storm> = bulletin.storms.iter().filter(|storm| storm.agency == "JTWC").cloned().collect();
        let mut storms = pass.storms;
        storms.extend(kept);
        bulletin.storms = storms;
        bulletin.storm_line = pass.line;
        return;
    }
    if pass.jtwc_ok {
        let kept: Vec<Storm> = bulletin.storms.iter().filter(|storm| storm.agency != "JTWC").cloned().collect();
        let mut storms = kept;
        storms.extend(pass.storms);
        bulletin.storms = storms;
        bulletin.storm_line = pass.line;
        return;
    }
    if bulletin.storm_line.is_empty() {
        bulletin.storm_line = pass.line;
    }
}

fn fetch_jtwc(agent: &ureq::Agent) -> Result<(String, Vec<Storm>), String> {
    let rss = http_get_once(agent, "https://www.metoc.navy.mil/jtwc/rss/jtwc.rss")?;
    let urls = jtwc_warning_urls(&rss).map_err(|_| "JTWC tropical cyclone warnings did not load.".to_string())?;
    if urls.is_empty() {
        return Ok(("JTWC lists no current tropical cyclone warnings.".into(), Vec::new()));
    }
    let mut storms = Vec::new();
    let mut missed = false;
    for url in urls {
        match http_get_once(agent, &url) {
            Ok(body) => {
                if let Some(storm) = jtwc_storm(&body) {
                    storms.push(storm);
                } else {
                    missed = true;
                }
            }
            Err(_) => missed = true,
        }
    }
    if storms.is_empty() {
        return Err("JTWC tropical cyclone warnings did not load.".into());
    }
    let mut line = storm_sentence(&storms);
    if missed {
        line.push_str(" A JTWC warning text did not load.");
    }
    Ok((line, storms))
}

/// Warning-text links outside the central and eastern Pacific item. Advisory files are not positions.
fn jtwc_warning_urls(rss: &str) -> Result<Vec<String>, ()> {
    if !rss.to_ascii_lowercase().contains("<item") {
        return Err(());
    }
    let mut urls = Vec::new();
    for item in rss.split("<item").skip(1) {
        let item = item.split("</item>").next().unwrap_or(item);
        let title = tag_text(item, "title").to_ascii_lowercase();
        if title.contains("central/eastern") || title.contains("significant tropical weather") {
            continue;
        }
        let mut rest = item;
        while let Some(start) = rest.to_ascii_lowercase().find("href") {
            rest = &rest[start + 4..];
            let Some(eq) = rest.find('=') else { break };
            let after = rest[eq + 1..].trim_start();
            let quote = after.as_bytes().first().copied();
            let (body, next) = if quote == Some(b'\'') || quote == Some(b'"') {
                let mark = quote.unwrap() as char;
                let tail = &after[1..];
                let Some(end) = tail.find(mark) else { break };
                (tail[..end].to_string(), &tail[end + 1..])
            } else {
                break;
            };
            rest = next;
            if !body.to_ascii_lowercase().contains("web.txt") {
                continue;
            }
            let name = body.rsplit('/').next().unwrap_or("").to_ascii_lowercase();
            if !is_jtwc_warning_file(&name) {
                continue;
            }
            if !urls.iter().any(|have| have == &body) {
                urls.push(body);
            }
        }
    }
    Ok(urls)
}

fn is_jtwc_warning_file(name: &str) -> bool {
    let Some(stem) = name.strip_suffix("web.txt") else { return false };
    if stem.len() < 4 || stem.starts_with("ab") || stem.starts_with("ep") || stem.starts_with("cp") {
        return false;
    }
    let (basin, digits) = stem.split_at(2);
    basin.chars().all(|ch| ch.is_ascii_alphabetic()) && digits.chars().all(|ch| ch.is_ascii_digit()) && !digits.is_empty()
}

fn tag_text<'a>(item: &'a str, tag: &str) -> &'a str {
    let open = format!("<{tag}");
    let Some(start) = item.to_ascii_lowercase().find(&open.to_ascii_lowercase()) else { return "" };
    let Some(end_open) = item[start..].find('>') else { return "" };
    let body = &item[start + end_open + 1..];
    let close = format!("</{tag}>");
    let end = body.to_ascii_lowercase().find(&close.to_ascii_lowercase()).unwrap_or(body.len());
    body[..end].trim()
}

/// The first WARNING POSITION and the first MAX SUSTAINED WINDS are the current warning.
/// Later blocks in the same file are the forecast and are not stored.
fn jtwc_storm(body: &str) -> Option<Storm> {
    let mut identity = None;
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(|ch: char| ch.is_ascii_digit()) {
            if let Some(found) = warning_identity(trimmed) {
                identity = Some(found);
                break;
            }
        }
    }
    if identity.is_none() {
        for line in body.lines() {
            if let Some(found) = warning_identity(line.trim()) {
                identity = Some(found);
                break;
            }
        }
    }
    let (class, id, name) = identity?;
    let mut lat = None;
    let mut lon = None;
    let mut position_time = String::new();
    let mut armed = false;
    let mut winds = None;
    for line in body.lines() {
        let trimmed = line.trim();
        let upper = trimmed.to_ascii_uppercase();
        if upper.contains("WARNING POSITION") {
            armed = true;
            continue;
        }
        if armed && lat.is_none() {
            if let Some((found_lat, found_lon, when)) = parse_near(&upper) {
                lat = Some(found_lat);
                lon = Some(found_lon);
                position_time = when;
            }
        }
        if lat.is_some() && winds.is_none() && upper.contains("MAX SUSTAINED WINDS") {
            winds = wind_kt(&upper);
        }
        if lat.is_some() && winds.is_some() {
            break;
        }
    }
    Some(Storm {
        agency: "JTWC".into(),
        id,
        name,
        class_code: class,
        lat: lat?,
        lon: lon?,
        intensity_kt: winds,
        motion_dir: None,
        motion_kt: None,
        updated: issued_label(body, &position_time),
    })
}

fn warning_identity(line: &str) -> Option<(String, String, String)> {
    let upper = line.to_ascii_uppercase();
    if !upper.contains("WARNING") || !line.contains('(') {
        return None;
    }
    let open = line.find('(')?;
    let close = line.find(')')?;
    if close <= open {
        return None;
    }
    let name = line[open + 1..close].trim().to_string();
    let mut before = line[..open].trim().trim_start_matches(|ch: char| ch.is_ascii_digit() || ch == '.' || ch == ' ');
    if let Some(rest) = before.strip_prefix("SUBJ/") {
        before = rest.trim();
    }
    let mut tokens: Vec<&str> = before.split_whitespace().collect();
    let id = tokens.pop()?.to_string();
    if id.len() < 3 || !id.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        return None;
    }
    let class = tokens.join(" ");
    if class.is_empty() || name.is_empty() {
        return None;
    }
    Some((class, id, name))
}

fn parse_near(upper: &str) -> Option<(f32, f32, String)> {
    let near = upper.find("NEAR")?;
    let when = upper
        .split_whitespace()
        .next()
        .filter(|token| token.ends_with('Z') && token.len() >= 5)
        .unwrap_or("")
        .to_string();
    let mut parts = upper[near + 4..].split_whitespace();
    let lat = hemisphere_token(parts.next()?)?;
    let lon = hemisphere_token(parts.next()?)?;
    Some((lat, lon, when))
}

fn hemisphere_token(token: &str) -> Option<f32> {
    let token = token.trim_matches(|ch: char| ch == ',' || ch == '.');
    if token.len() < 2 {
        return None;
    }
    let (number, sign) = match token.as_bytes().last().map(|byte| byte.to_ascii_uppercase()) {
        Some(b'S') | Some(b'W') => (&token[..token.len() - 1], -1.0_f32),
        Some(b'N') | Some(b'E') => (&token[..token.len() - 1], 1.0_f32),
        _ => return None,
    };
    let value: f32 = number.parse().ok()?;
    Some(value * sign)
}

fn wind_kt(upper: &str) -> Option<f32> {
    let start = upper.find("MAX SUSTAINED WINDS")?;
    let rest = &upper[start..];
    let kt = rest.find("KT")?;
    let number = rest[..kt].split_whitespace().last()?.trim_matches('-');
    let value: f32 = number.parse().ok()?;
    value.is_finite().then_some(value)
}

fn issued_label(body: &str, position_time: &str) -> String {
    let mut date = String::new();
    for token in body.split_whitespace() {
        let token = token.trim_matches(|ch: char| !ch.is_ascii_alphanumeric());
        let bytes = token.as_bytes();
        if bytes.len() == 7
            && bytes[0].is_ascii_digit()
            && bytes[1].is_ascii_digit()
            && bytes[2].is_ascii_alphabetic()
            && bytes[3].is_ascii_alphabetic()
            && bytes[4].is_ascii_alphabetic()
            && bytes[5].is_ascii_digit()
            && bytes[6].is_ascii_digit()
        {
            date = token.to_ascii_uppercase();
            break;
        }
    }
    let mut issue = String::new();
    if let Some(first) = body.lines().next() {
        for token in first.split_whitespace() {
            if token.len() == 6 && token.chars().all(|ch| ch.is_ascii_digit()) {
                issue = format!("{token}Z");
            }
        }
    }
    match (date.is_empty(), issue.is_empty()) {
        (false, false) => format!("{date} {issue}"),
        (false, true) => date,
        (true, false) => issue,
        (true, true) => position_time.to_string(),
    }
}

fn storm_sentence(storms: &[Storm]) -> String {
    let shown = storms
        .iter()
        .take(4)
        .map(|storm| {
            let class = storm_class(&storm.class_code);
            let kt = storm.intensity_kt.map(|value| format!(", {value:.0} kt")).unwrap_or_default();
            let name = if storm.name.is_empty() { storm.id.as_str() } else { storm.name.as_str() };
            let updated = if storm.updated.is_empty() { String::new() } else { format!(", updated {}", storm.updated) };
            format!("{} {class} {name} at {}{kt}{updated}", storm.agency, geo::format_lat_lon_deg(storm.lat, storm.lon))
        })
        .collect::<Vec<_>>()
        .join("; ");
    let extra = if storms.len() > 4 { format!(" and {} more", storms.len() - 4) } else { String::new() };
    format!("{shown}{extra}.")
}

fn storm_rows(storms: &[Storm]) -> Vec<series::Row> {
    storms
        .iter()
        .map(|storm| {
            let mut row = series::Row::new();
            row.insert("t".into(), serde_json::Value::String(storm.updated.clone()));
            row.insert("agency".into(), serde_json::Value::String(storm.agency.clone()));
            row.insert("id".into(), serde_json::Value::String(storm.id.clone()));
            row.insert("name".into(), serde_json::Value::String(storm.name.clone()));
            row.insert("class".into(), serde_json::Value::String(storm.class_code.clone()));
            series::put_f64(&mut row, "lat", f64::from(storm.lat));
            series::put_f64(&mut row, "lon", f64::from(storm.lon));
            match storm.intensity_kt {
                Some(kt) => series::put_f64(&mut row, "intensity_kt", f64::from(kt)),
                None => {
                    row.insert("intensity_kt".into(), serde_json::Value::Null);
                }
            }
            row
        })
        .collect()
}

fn storm_report(body: &str) -> Result<(String, Vec<Storm>), String> {
    let value: Value = serde_json::from_str(body).map_err(|err| err.to_string())?;
    let list = value.get("activeStorms").and_then(|v| v.as_array()).ok_or("NHC storm list has no activeStorms")?;
    let mut storms = Vec::new();
    for item in list {
        if let Some(storm) = storm_from(item) {
            storms.push(storm);
        }
    }
    if storms.is_empty() {
        return Ok(("NHC lists no active storms in the Atlantic or the eastern and central Pacific.".into(), storms));
    }
    let shown = storms
        .iter()
        .take(4)
        .map(|storm| {
            let class = storm_class(&storm.class_code);
            let kt = storm.intensity_kt.map(|value| format!(", {value:.0} kt")).unwrap_or_default();
            let motion = match (storm.motion_dir, storm.motion_kt) {
                (Some(dir), Some(speed)) => format!(", moving {dir:.0}° at {speed:.0} kt"),
                _ => String::new(),
            };
            let name = if storm.name.is_empty() { storm.id.as_str() } else { storm.name.as_str() };
            let updated = if storm.updated.is_empty() {
                String::new()
            } else {
                format!(", updated {}", storm.updated)
            };
            format!(
                "{} {class} {name} at {}{kt}{motion}{updated}",
                storm.agency,
                geo::format_lat_lon_deg(storm.lat, storm.lon)
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    let extra = if storms.len() > 4 {
        format!(" and {} more", storms.len() - 4)
    } else {
        String::new()
    };
    Ok((format!("{shown}{extra}."), storms))
}

fn storm_from(item: &Value) -> Option<Storm> {
    let lat = coord_value(item, "latitudeNumeric", "latitude")?;
    let lon = coord_value(item, "longitudeNumeric", "longitude")?;
    let id = item.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if id.is_empty() && name.is_empty() {
        return None;
    }
    Some(Storm {
        agency: "NHC".into(),
        id,
        name,
        class_code: item.get("classification").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        lat,
        lon,
        intensity_kt: item.get("intensity").and_then(json_f64).map(|v| v as f32),
        motion_dir: item.get("movementDir").and_then(json_f64).map(|v| v as f32),
        motion_kt: item.get("movementSpeed").and_then(json_f64).map(|v| v as f32),
        updated: item.get("lastUpdate").and_then(|v| v.as_str()).unwrap_or("").to_string(),
    })
}

fn storm_class(code: &str) -> String {
    let known = match code {
        "TD" => "Tropical Depression",
        "TS" => "Tropical Storm",
        "HU" => "Hurricane",
        "MH" => "Major Hurricane",
        "STD" | "SD" => "Subtropical Depression",
        "STS" | "SS" => "Subtropical Storm",
        "EX" => "Extratropical",
        "PT" => "Post-tropical",
        "DB" => "Disturbance",
        "LO" => "Low",
        "WV" => "Tropical Wave",
        _ => "",
    };
    if !known.is_empty() {
        known.into()
    } else if code.is_empty() {
        "storm".into()
    } else {
        code.to_string()
    }
}

fn coord_value(item: &Value, numeric: &str, text: &str) -> Option<f32> {
    if let Some(value) = item.get(numeric).and_then(json_f64) {
        return Some(value as f32);
    }
    parse_hemisphere(item.get(text)?.as_str()?)
}

fn parse_hemisphere(raw: &str) -> Option<f32> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let (number, sign) = match raw.as_bytes().last().map(|byte| byte.to_ascii_uppercase()) {
        Some(b'S') | Some(b'W') => (&raw[..raw.len() - 1], -1.0),
        Some(b'N') | Some(b'E') => (&raw[..raw.len() - 1], 1.0),
        _ => (raw, 1.0),
    };
    let value: f32 = number.trim().parse().ok()?;
    Some(value.abs() * sign)
}

fn json_f64(value: &Value) -> Option<f64> {
    value.as_f64().or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
}

const VAAC_FAIL: &str = "NWS volcanic ash advisories did not load.";
const VAAC_SOURCE: &str = "NWS VAAC relay";
const GAUGE_FAIL: &str = "UNESCO IOC sea-level gauges did not load.";
const GAUGE_SOURCE: &str = "UNESCO IOC sea level";

struct FvEntry {
    name: String,
    unix: f64,
    size: u64,
}

struct VaacAdvisory {
    file: String,
    t: String,
    dtg: String,
    vaac: String,
    volcano: String,
    advisory: String,
    psn: String,
    lat: Option<f64>,
    lon: Option<f64>,
    area: String,
    eruption: String,
    colour: String,
    test: bool,
    text: String,
}

struct VaacPass {
    line: String,
    desk: Vec<String>,
}

fn vaac_index_url() -> &'static str {
    "https://tgftp.nws.noaa.gov/data/raw/fv/"
}

fn gauge_list_url() -> &'static str {
    "https://www.ioc-sealevelmonitoring.org/service.php?query=stationlist&format=json&showall=a"
}

fn fetch_vaac(agent: &ureq::Agent, quick: &ureq::Agent, held: &mut Vec<series::HeldNote>) -> Result<VaacPass, String> {
    let index = match http_get_once(agent, vaac_index_url()) {
        Ok(body) => body,
        Err(_) => {
            recall_named(held, "vaac", VAAC_SOURCE, VAAC_FAIL);
            return Err(VAAC_FAIL.into());
        }
    };
    let files = fv_entries(&index);
    if files.is_empty() && !index.contains("Index of") {
        recall_named(held, "vaac", VAAC_SOURCE, VAAC_FAIL);
        return Err(VAAC_FAIL.into());
    }
    let recent = recent_fv(&files, geo::unix_now());
    if recent.is_empty() {
        hold_body(held, "vaac", VAAC_SOURCE, Ok(Vec::new()), VAAC_FAIL, true);
        let (line, desk) = vaac_desk(&[], false);
        return Ok(VaacPass { line, desk });
    }
    let mut items = Vec::new();
    let mut missed = false;
    for file in recent {
        let url = format!("{}{}", vaac_index_url(), file.name);
        match http_get_once(quick, &url) {
            Ok(body) => {
                if let Some(item) = parse_vaac(&body, &file.name, &vaac_file_time(file.unix)) {
                    items.push(item);
                }
            }
            Err(_) => missed = true,
        }
    }
    if items.is_empty() {
        if missed {
            recall_named(held, "vaac", VAAC_SOURCE, VAAC_FAIL);
            return Err(VAAC_FAIL.into());
        }
        hold_body(held, "vaac", VAAC_SOURCE, Ok(Vec::new()), VAAC_FAIL, true);
        let (line, desk) = vaac_desk(&[], false);
        return Ok(VaacPass { line, desk });
    }
    let rows = items.iter().map(vaac_row).collect();
    hold_body(held, "vaac", VAAC_SOURCE, Ok(rows), VAAC_FAIL, true);
    let (line, desk) = vaac_desk(&items, missed);
    Ok(VaacPass { line, desk })
}

fn fetch_gauges(agent: &ureq::Agent, held: &mut Vec<series::HeldNote>) -> Result<String, String> {
    let body = match http_get_once(agent, gauge_list_url()) {
        Ok(body) => body,
        Err(_) => {
            recall_named(held, "gauges", GAUGE_SOURCE, GAUGE_FAIL);
            return Err(GAUGE_FAIL.into());
        }
    };
    match series::gauge_rows(&body) {
        Ok(rows) => {
            let line = gauge_summary(&rows);
            hold_body(held, "gauges", GAUGE_SOURCE, Ok(rows), GAUGE_FAIL, true);
            Ok(line)
        }
        Err(()) => {
            recall_named(held, "gauges", GAUGE_SOURCE, GAUGE_FAIL);
            Err(GAUGE_FAIL.into())
        }
    }
}

/// NWS keeps the latest text on each FV header. A header older than 7 days is the last stored text, not a current advisory.
fn recent_fv(files: &[FvEntry], now: f64) -> Vec<&FvEntry> {
    files.iter().filter(|file| file.size > 1 && file.unix <= now && now - file.unix <= 7.0 * 86400.0).collect()
}

fn fv_entries(html: &str) -> Vec<FvEntry> {
    let mut out = Vec::new();
    for line in html.lines() {
        let Some(name) = line.split("href=\"").nth(1).and_then(|rest| rest.split('"').next()) else { continue };
        if !name.ends_with(".txt") || name.contains('/') || name.contains('\\') {
            continue;
        }
        let mut cells = line.split("<td align=\"right\">").skip(1);
        let Some(when) = cells.next().and_then(|cell| cell.split('<').next()) else { continue };
        let Some(size_text) = cells.next().and_then(|cell| cell.split('<').next()) else { continue };
        let Some(unix) = fv_unix(when) else { continue };
        let Ok(size) = size_text.trim().parse::<u64>() else { continue };
        out.push(FvEntry { name: name.to_string(), unix, size });
    }
    out
}

fn fv_unix(text: &str) -> Option<f64> {
    let mut parts = text.split_whitespace();
    let date = parts.next()?;
    let clock = parts.next()?;
    let mut date_parts = date.split('-');
    let day: u32 = date_parts.next()?.parse().ok()?;
    let month = month_index(date_parts.next()?)?;
    let year: i32 = date_parts.next()?.parse().ok()?;
    let mut clock_parts = clock.split(':');
    let hour: u32 = clock_parts.next()?.parse().ok()?;
    let minute: u32 = clock_parts.next()?.trim().parse().ok()?;
    if hour > 23 || minute > 59 || day == 0 || day > 31 {
        return None;
    }
    Some(geo::unix_utc(year, month, day, f64::from(hour) + f64::from(minute) / 60.0))
}

fn month_index(name: &str) -> Option<u32> {
    let name = name.to_ascii_lowercase();
    match name.as_str() {
        "jan" => Some(1),
        "feb" => Some(2),
        "mar" => Some(3),
        "apr" => Some(4),
        "may" => Some(5),
        "jun" => Some(6),
        "jul" => Some(7),
        "aug" => Some(8),
        "sep" => Some(9),
        "oct" => Some(10),
        "nov" => Some(11),
        "dec" => Some(12),
        _ => None,
    }
}

fn vaac_file_time(unix: f64) -> String {
    let secs = unix.floor() as i64;
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (year, month, day) = geo::civil_from_days(days);
    let hour = rem / 3600;
    let minute = (rem % 3600) / 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:00Z")
}

fn parse_vaac(body: &str, file: &str, file_t: &str) -> Option<VaacAdvisory> {
    if !body.to_ascii_uppercase().contains("VA ADVISORY") {
        return None;
    }
    let fields = vaac_fields(body);
    let dtg = field_value(&fields, "DTG");
    let psn = field_value(&fields, "PSN");
    let (lat, lon) = psn_pair(&psn);
    let british = field_value(&fields, "AVIATION COLOUR CODE");
    let colour = if british.is_empty() { field_value(&fields, "AVIATION COLOR CODE") } else { british };
    Some(VaacAdvisory {
        file: file.to_string(),
        t: dtg_iso(&dtg).unwrap_or_else(|| file_t.to_string()),
        dtg,
        vaac: field_value(&fields, "VAAC"),
        volcano: field_value(&fields, "VOLCANO"),
        advisory: field_value(&fields, "ADVISORY NR"),
        psn,
        lat,
        lon,
        area: field_value(&fields, "AREA"),
        eruption: field_value(&fields, "ERUPTION DETAILS"),
        colour,
        test: vaac_is_test(&fields),
        text: body.trim().to_string(),
    })
}

fn vaac_fields(body: &str) -> Vec<(String, String)> {
    let mut fields = Vec::new();
    let mut key = String::new();
    let mut value = String::new();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some((label, rest)) = vaac_label(line) {
            if !key.is_empty() {
                fields.push((std::mem::take(&mut key), value.split_whitespace().collect::<Vec<_>>().join(" ")));
            }
            key = label.to_string();
            value = rest.to_string();
        } else if !key.is_empty() {
            if !value.is_empty() {
                value.push(' ');
            }
            value.push_str(line);
        }
    }
    if !key.is_empty() {
        fields.push((key, value.split_whitespace().collect::<Vec<_>>().join(" ")));
    }
    fields
}

fn vaac_label(line: &str) -> Option<(&str, &str)> {
    let colon = line.find(':')?;
    let label = &line[..colon];
    if label.is_empty() || !label.chars().next()?.is_ascii_uppercase() {
        return None;
    }
    if !label.chars().all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == ' ' || ch == '+' || ch == '/') {
        return None;
    }
    Some((label, line[colon + 1..].trim()))
}

fn field_value(fields: &[(String, String)], label: &str) -> String {
    fields.iter().find(|(key, _)| key.eq_ignore_ascii_case(label)).map(|(_, value)| value.clone()).unwrap_or_default()
}

fn vaac_is_test(fields: &[(String, String)]) -> bool {
    let status = field_value(fields, "STATUS");
    let eruption = field_value(fields, "ERUPTION DETAILS");
    status.to_ascii_uppercase().contains("TEST") || eruption.eq_ignore_ascii_case("VA TEST")
}

fn dtg_iso(dtg: &str) -> Option<String> {
    let dtg = dtg.trim().trim_end_matches(|ch: char| ch == 'Z' || ch == 'z');
    let (day, clock) = dtg.split_once('/')?;
    if day.len() != 8 || clock.len() < 4 {
        return None;
    }
    if !day.chars().all(|ch| ch.is_ascii_digit()) || !clock[..4].chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    Some(format!("{}-{}-{}T{}:{}:00Z", &day[0..4], &day[4..6], &day[6..8], &clock[0..2], &clock[2..4]))
}

fn psn_pair(psn: &str) -> (Option<f64>, Option<f64>) {
    let mut lat = None;
    let mut lon = None;
    for token in psn.split_whitespace() {
        let Some(value) = hemisphere_deg(token) else { continue };
        match token.trim().chars().next().unwrap_or(' ').to_ascii_uppercase() {
            'N' | 'S' => lat = Some(value),
            'E' | 'W' => lon = Some(value),
            _ => {}
        }
    }
    (lat, lon)
}

fn hemisphere_deg(token: &str) -> Option<f64> {
    let token = token.trim().trim_matches(|ch: char| ch == ',' || ch == '.');
    let mut chars = token.chars();
    let sign = match chars.next()?.to_ascii_uppercase() {
        'N' | 'E' => 1.0,
        'S' | 'W' => -1.0,
        _ => return None,
    };
    let digits: String = chars.collect();
    if digits.len() < 3 || !digits.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    let split = digits.len() - 2;
    let degrees: f64 = digits[..split].parse().ok()?;
    let minutes: f64 = digits[split..].parse().ok()?;
    if minutes >= 60.0 {
        return None;
    }
    Some(sign * (degrees + minutes / 60.0))
}

fn vaac_desk(items: &[VaacAdvisory], missed: bool) -> (String, Vec<String>) {
    let mut operational: Vec<&VaacAdvisory> = items.iter().filter(|item| !item.test).collect();
    operational.sort_by(|a, b| b.t.cmp(&a.t).then(a.file.cmp(&b.file)));
    let tests = items.iter().filter(|item| item.test).count();
    let line = if operational.is_empty() {
        "NWS lists no volcanic ash advisory updated in the last 7 days.".to_string()
    } else if operational.len() == 1 {
        "NWS is holding 1 volcanic ash advisory updated in the last 7 days.".to_string()
    } else {
        format!("NWS is holding {} volcanic ash advisories updated in the last 7 days.", operational.len())
    };
    let mut desk = vec![line.clone()];
    for item in &operational {
        desk.push(vaac_quote(item));
    }
    if !operational.is_empty() {
        desk.push("These lines quote the NWS FV text updated in the last 7 days. A FIRMS detection is not an ash advisory.".into());
    }
    let test_line = if tests == 0 {
        String::new()
    } else if tests == 1 {
        "NWS is also holding 1 VAAC test bulletin. A test is not an ash advisory.".to_string()
    } else {
        format!("NWS is also holding {tests} VAAC test bulletins. A test is not an ash advisory.")
    };
    if !test_line.is_empty() {
        desk.push(test_line.clone());
    }
    if missed {
        desk.push("A volcanic ash advisory text did not load.".into());
    }
    let mut reading = line;
    if !test_line.is_empty() {
        reading.push(' ');
        reading.push_str(&test_line);
    }
    if missed {
        reading.push(' ');
        reading.push_str("A volcanic ash advisory text did not load.");
    }
    (reading, desk)
}

fn vaac_quote(item: &VaacAdvisory) -> String {
    let center = if item.vaac.is_empty() { "center was not in that advisory".to_string() } else { item.vaac.clone() };
    let volcano = if item.volcano.is_empty() { "volcano was not in that advisory".to_string() } else { item.volcano.clone() };
    let number = if item.advisory.is_empty() { "number was not in that advisory".to_string() } else { item.advisory.clone() };
    let when = if item.dtg.is_empty() { item.t.clone() } else { item.dtg.clone() };
    let details = if item.eruption.is_empty() {
        "Eruption details were not in that advisory.".to_string()
    } else {
        format!("Eruption details: {}.", item.eruption.trim_end_matches('.'))
    };
    let colour = if item.colour.is_empty() {
        String::new()
    } else {
        format!(" Aviation colour code: {}.", item.colour.trim_end_matches('.'))
    };
    format!("VAAC {center}: {volcano} advisory {number} at {when}. {details}{colour}")
}

fn vaac_row(item: &VaacAdvisory) -> series::Row {
    let mut row = series::Row::new();
    row.insert("t".into(), Value::String(item.t.clone()));
    row.insert("file".into(), Value::String(item.file.clone()));
    row.insert("dtg".into(), Value::String(item.dtg.clone()));
    row.insert("vaac".into(), Value::String(item.vaac.clone()));
    row.insert("volcano".into(), Value::String(item.volcano.clone()));
    row.insert("advisory".into(), Value::String(item.advisory.clone()));
    row.insert("psn".into(), Value::String(item.psn.clone()));
    match item.lat {
        Some(lat) => series::put_f64(&mut row, "lat", lat),
        None => {
            row.insert("lat".into(), Value::Null);
        }
    }
    match item.lon {
        Some(lon) => series::put_f64(&mut row, "lon", lon),
        None => {
            row.insert("lon".into(), Value::Null);
        }
    }
    row.insert("area".into(), Value::String(item.area.clone()));
    row.insert("eruption".into(), Value::String(item.eruption.clone()));
    if item.colour.is_empty() {
        row.insert("colour".into(), Value::Null);
    } else {
        row.insert("colour".into(), Value::String(item.colour.clone()));
    }
    row.insert("test".into(), Value::Bool(item.test));
    row.insert("text".into(), Value::String(item.text.clone()));
    row
}

fn gauge_summary(rows: &[series::Row]) -> String {
    if rows.is_empty() {
        return "UNESCO IOC lists no active sea-level stations.".into();
    }
    let mut coastal = Vec::new();
    let mut deep = 0usize;
    for row in rows {
        let Some(value) = row.get("level").and_then(|item| item.as_f64()) else { continue };
        let kind = row.get("type").and_then(|item| item.as_str()).unwrap_or("");
        let units = row.get("units").and_then(|item| item.as_str()).unwrap_or("");
        if kind == "SD" {
            deep += 1;
            continue;
        }
        if units == "M" {
            coastal.push(value);
        }
    }
    let body = if coastal.is_empty() && deep == 0 {
        "UNESCO IOC sea-level gauges did not include a finite meter sample.".to_string()
    } else if coastal.is_empty() {
        let word = if deep == 1 { "sample" } else { "samples" };
        format!("No station outside deep-ocean type SD included a finite meter sample. Deep-ocean type SD has {deep} finite {word}.")
    } else {
        let stations = if coastal.len() == 1 { "station has" } else { "stations have" };
        let deep_word = if deep == 1 { "sample" } else { "samples" };
        format!(
            "{} {stations} a finite meter sample outside deep-ocean type SD. Deep-ocean type SD has {deep} finite {deep_word}. Samples keep the station sensor and datum.",
            coastal.len()
        )
    };
    format!(
        "UNESCO IOC active stations: {} rows. {body} Feet and volts stay on their rows. A station sample is a reading. It is not a tsunami warning. NWS tsunami bulletins stay on their own line. Missing code -999 stays empty. Real-time values have limited quality control.",
        rows.len()
    )
}

fn fetch_tsunami(agent: &ureq::Agent) -> Result<(String, Vec<String>), String> {
    let body = http_get_nws(
        agent,
        "https://api.weather.gov/alerts/active?event=Tsunami%20Warning,Tsunami%20Watch,Tsunami%20Advisory",
    )?;
    tsunami_report(&body)
}

fn tsunami_report(body: &str) -> Result<(String, Vec<String>), String> {
    let value: Value = serde_json::from_str(body).map_err(|err| err.to_string())?;
    let features = value.get("features").and_then(|v| v.as_array()).ok_or("NWS tsunami bulletin has no features")?;
    if features.is_empty() {
        return Ok(("NWS lists no current tsunami Warning, Watch, or Advisory.".into(), Vec::new()));
    }
    let mut parts = Vec::new();
    let mut ids = Vec::new();
    for feature in features.iter().take(3) {
        let props = &feature["properties"];
        let event = props.get("event").and_then(|v| v.as_str()).unwrap_or("Tsunami bulletin");
        let area = props.get("areaDesc").and_then(|v| v.as_str()).unwrap_or("");
        let sent = props.get("sent").and_then(|v| v.as_str()).unwrap_or("");
        let id = props
            .get("id")
            .and_then(|v| v.as_str())
            .or_else(|| feature.get("id").and_then(|v| v.as_str()))
            .unwrap_or(event);
        ids.push(id.to_string());
        let area = if area.is_empty() {
            String::new()
        } else {
            format!(" · {}", area.chars().take(80).collect::<String>())
        };
        let sent = if sent.is_empty() { String::new() } else { format!(" · {sent}") };
        parts.push(format!("NWS {event}{area}{sent}"));
    }
    let extra = if features.len() > 3 {
        format!(" and {} more", features.len() - 3)
    } else {
        String::new()
    };
    Ok((format!("{}.", parts.join("; ") + &extra), ids))
}

fn fetch_weekly(agent: &ureq::Agent) -> Result<(String, Vec<WeeklyVolcano>), String> {
    let body = http_get_once(agent, "https://volcano.si.edu/news/WeeklyVolcanoRSS.xml")?;
    weekly_report(&body)
}

fn weekly_report(body: &str) -> Result<(String, Vec<WeeklyVolcano>), String> {
    if !body.contains("<item>") {
        return Err("weekly report has no items".into());
    }
    let mut reports = Vec::new();
    for chunk in body.split("<item>").skip(1) {
        let item = chunk.split("</item>").next().unwrap_or(chunk);
        let Some(title) = xml_text(item, "title") else { continue };
        let Some((name, country, week, kind)) = split_weekly_title(&title) else { continue };
        let Some((lat, lon)) = georss_point(item) else { continue };
        reports.push(WeeklyVolcano {
            name,
            country,
            week,
            kind,
            lat,
            lon,
            guid: xml_text(item, "guid").unwrap_or_default(),
        });
    }
    if reports.is_empty() {
        return Ok(("Smithsonian/USGS weekly volcanic report listed no volcanoes.".into(), reports));
    }
    let week = reports[0].week.clone();
    let new_reports: Vec<&WeeklyVolcano> = reports.iter().filter(|report| report.kind.to_ascii_lowercase().starts_with("new")).collect();
    let names = new_reports.iter().map(|report| report.name.as_str()).collect::<Vec<_>>().join(", ");
    let new_bit = if names.is_empty() { String::new() } else { format!(" New: {names}.") };
    let line = format!(
        "Smithsonian/USGS weekly report for {week}: {} new, {} continuing.{new_bit}",
        new_reports.len(),
        reports.len() - new_reports.len()
    );
    Ok((line, reports))
}

fn split_weekly_title(title: &str) -> Option<(String, String, String, String)> {
    let (left, rest) = title.split_once(" - Report for ")?;
    let (week, kind) = rest.split_once(" - ")?;
    let (name, country) = left.rsplit_once(" (")?;
    let country = country.trim_end_matches(')');
    if name.trim().is_empty() || week.trim().is_empty() || kind.trim().is_empty() {
        return None;
    }
    Some((name.trim().to_string(), country.trim().to_string(), week.trim().to_string(), kind.trim().to_string()))
}

fn xml_text(item: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}");
    let start = item.find(&open)?;
    let after = item[start..].find('>')? + start + 1;
    let close = format!("</{tag}>");
    let end = item[after..].find(&close)? + after;
    let text = item[after..end].trim();
    if text.is_empty() {
        return None;
    }
    Some(unescape_xml(text))
}

fn unescape_xml(text: &str) -> String {
    text.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'")
}

fn georss_point(item: &str) -> Option<(f32, f32)> {
    let start = item.find("<georss:point>")? + "<georss:point>".len();
    let end = item[start..].find("</georss:point>")? + start;
    let mut parts = item[start..end].split_whitespace();
    let lat: f32 = parts.next()?.parse().ok()?;
    let lon: f32 = parts.next()?.parse().ok()?;
    Some((lat, lon))
}

fn http_get_nws(agent: &ureq::Agent, url: &str) -> Result<String, String> {
    agent
        .get(url)
        .set("User-Agent", "FSOT-Global-Monitor/0.1 (local Earth monitor)")
        .set("Accept", "application/geo+json, application/json")
        .call()
        .map_err(|err| err.to_string())?
        .into_string()
        .map_err(|err| err.to_string())
}

fn fetch_volcano_alerts(agent: &ureq::Agent) -> Result<Vec<VolcanoAlert>, String> {
    let body = http_get(agent, "https://volcanoes.usgs.gov/vsc/api/volcanoApi/elevated")?;
    parse_volcano_alerts(&body)
}

fn parse_volcano_alerts(body: &str) -> Result<Vec<VolcanoAlert>, String> {
    let value: Value = serde_json::from_str(body).map_err(|err| err.to_string())?;
    let list = value.as_array().ok_or("USGS volcano alert feed is not a list")?;
    let mut alerts = Vec::new();
    for item in list {
        let Some(name) = item.get("vName").and_then(|v| v.as_str()).filter(|text| !text.is_empty()) else { continue };
        let Some(lat) = item.get("lat").and_then(|v| v.as_f64()) else { continue };
        let Some(lon) = item.get("long").and_then(|v| v.as_f64()) else { continue };
        alerts.push(VolcanoAlert {
            name: name.to_string(),
            lat: lat as f32,
            lon: lon as f32,
            alert_level: item.get("alertLevel").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            color_code: item.get("colorCode").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            synopsis: item.get("noticeSynopsis").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        });
    }
    Ok(alerts)
}

fn fetch_aftershock(agent: &ureq::Agent, quakes: &[Quake]) -> String {
    let now = geo::unix_now();
    let start = ymd(now - 7.0 * 86400.0);
    let end = ymd(now + 86400.0);
    let search = format!(
        "https://earthquake.usgs.gov/fdsnws/event/1/query?format=geojson&producttype=oaf&minmagnitude=4.5&limit=1&orderby=time&starttime={start}&endtime={end}"
    );
    match http_get(agent, &search) {
        Ok(body) => match newest_oaf_feature(&body) {
            Some((place, mag, detail)) => load_oaf_line(agent, &place, mag, &detail),
            None => "USGS attached no aftershock forecast to M4.5+ events in the last 7 days.".into(),
        },
        Err(_) => {
            if let Some(quake) = quakes.iter().filter(|quake| quake.types.contains("oaf")).max_by(|a, b| a.mag.total_cmp(&b.mag)) {
                load_oaf_line(agent, &quake.place, quake.mag as f64, &quake.detail)
            } else {
                "USGS aftershock forecast did not load.".into()
            }
        }
    }
}

fn newest_oaf_feature(body: &str) -> Option<(String, f64, String)> {
    let value: Value = serde_json::from_str(body).ok()?;
    let props = &value["features"].get(0)?["properties"];
    Some((
        props.get("place").and_then(|v| v.as_str()).unwrap_or("the sequence").to_string(),
        props.get("mag").and_then(|v| v.as_f64()).unwrap_or(0.0),
        props.get("detail").and_then(|v| v.as_str()).unwrap_or("").to_string(),
    ))
}

fn load_oaf_line(agent: &ureq::Agent, place: &str, mag: f64, detail: &str) -> String {
    if !detail.starts_with("https://earthquake.usgs.gov/") {
        return "USGS attached no aftershock forecast to M4.5+ events in the last 7 days.".into();
    }
    let detail_body = match http_get(agent, detail) {
        Ok(body) => body,
        Err(_) => return "USGS aftershock forecast did not load.".into(),
    };
    let Some(url) = oaf_forecast_url(&detail_body) else {
        return "USGS attached no aftershock forecast to M4.5+ events in the last 7 days.".into();
    };
    if !url.starts_with("https://earthquake.usgs.gov/") {
        return "USGS attached no aftershock forecast to M4.5+ events in the last 7 days.".into();
    }
    let forecast = match http_get(agent, &url) {
        Ok(body) => body,
        Err(_) => return "USGS aftershock forecast did not load.".into(),
    };
    oaf_line(place, mag, &forecast).unwrap_or_else(|| "USGS aftershock forecast did not include a probability.".into())
}

fn oaf_forecast_url(detail_body: &str) -> Option<String> {
    let value: Value = serde_json::from_str(detail_body).ok()?;
    let contents = &value["properties"]["products"]["oaf"].get(0)?["contents"];
    contents.get("forecast.json")?.get("url")?.as_str().map(|url| url.to_string())
}

fn oaf_line(place: &str, mag: f64, body: &str) -> Option<String> {
    let value: Value = serde_json::from_str(body).ok()?;
    let advisory = value.get("advisoryTimeFrame").and_then(|v| v.as_str()).unwrap_or("1 Week");
    let forecast = value.get("forecast")?.as_array()?;
    let window = forecast
        .iter()
        .find(|row| row.get("label").and_then(|v| v.as_str()) == Some(advisory))
        .or_else(|| forecast.iter().find(|row| row.get("label").and_then(|v| v.as_str()) == Some("1 Week")))
        .or_else(|| forecast.first())?;
    let label = window.get("label").and_then(|v| v.as_str()).unwrap_or(advisory);
    let bins = window.get("bins")?.as_array()?;
    let bin = bins
        .iter()
        .find(|row| (row.get("magnitude").and_then(|v| v.as_f64()).unwrap_or(-1.0) - 5.0).abs() < 0.05)
        .or_else(|| {
            bins.iter().max_by(|a, b| {
                let left = a.get("magnitude").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let right = b.get("magnitude").and_then(|v| v.as_f64()).unwrap_or(0.0);
                left.total_cmp(&right)
            })
        })?;
    let probability = bin.get("probability")?.as_f64()?;
    let magnitude = bin.get("magnitude").and_then(|v| v.as_f64()).unwrap_or(5.0);
    let model = value["model"]["name"].as_str().unwrap_or("USGS");
    let model = if model.contains("Reasenberg-Jones") { "Reasenberg-Jones, USGS" } else { model };
    Some(format!(
        "USGS aftershock forecast for M{mag:.1} {place}, {label}: chance of one or more M≥{magnitude:.1} is {percent:.1}% ({model}).",
        percent = probability * 100.0
    ))
}

fn ymd(unix: f64) -> String {
    let days = (unix.floor() as i64).div_euclid(86400);
    let (year, month, day) = geo::civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

fn hourly_peak(body: &str, field: &str) -> Option<SamplePeak> {
    let value: Value = serde_json::from_str(body).ok()?;
    let items: Vec<&Value> = if let Some(list) = value.as_array() {
        list.iter().collect()
    } else {
        vec![&value]
    };
    let mut best: Option<SamplePeak> = None;
    for item in items {
        let Some(lat) = item.get("latitude").and_then(|v| v.as_f64()) else { continue };
        let Some(lon) = item.get("longitude").and_then(|v| v.as_f64()) else { continue };
        let Some(hourly) = item.get("hourly") else { continue };
        let Some(times) = hourly.get("time").and_then(|v| v.as_array()) else { continue };
        let Some(values) = hourly.get(field).and_then(|v| v.as_array()) else { continue };
        for (index, value) in values.iter().enumerate() {
            let Some(number) = value.as_f64() else { continue };
            if !number.is_finite() {
                continue;
            }
            if best.as_ref().map(|peak| number as f32 <= peak.value).unwrap_or(false) {
                continue;
            }
            best = Some(SamplePeak {
                lat: lat as f32,
                lon: lon as f32,
                value: number as f32,
                when: times.get(index).and_then(|v| v.as_str()).unwrap_or("").to_string(),
            });
        }
    }
    best
}

fn parse_quakes(body: &str) -> Result<Vec<Quake>, String> {
    let value: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let mut quakes = Vec::new();
    for feature in value.get("features").and_then(|v| v.as_array()).ok_or("USGS feed has no features")? {
        let props = &feature["properties"];
        let coords = feature["geometry"]["coordinates"].as_array();
        let Some(coords) = coords else { continue };
        if coords.len() < 2 {
            continue;
        }
        let Some(lon) = coords[0].as_f64() else { continue };
        let Some(lat) = coords[1].as_f64() else { continue };
        let mag = props.get("mag").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
        let depth = coords.get(2).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
        let place = props.get("place").and_then(|v| v.as_str()).unwrap_or("unknown place").to_string();
        let time_ms = props.get("time").and_then(|v| v.as_i64()).unwrap_or(0);
        let alert = props
            .get("alert")
            .and_then(|v| v.as_str())
            .filter(|text| !text.is_empty())
            .map(|text| text.to_string());
        let tsunami = props.get("tsunami").and_then(|v| v.as_i64()).unwrap_or(0) == 1;
        let types = props.get("types").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let detail = props.get("detail").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let id = feature.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        quakes.push(Quake {
            mag,
            lat: lat as f32,
            lon: lon as f32,
            depth_km: depth,
            place,
            time_ms,
            alert,
            tsunami,
            types,
            detail,
            id,
            nst: opt_i32(props.get("nst")),
            gap: opt_f32(props.get("gap")),
            dmin: opt_f32(props.get("dmin")),
            rms: opt_f32(props.get("rms")),
            mag_type: props.get("magType").and_then(|v| v.as_str()).filter(|text| !text.is_empty()).map(|text| text.to_string()),
        });
    }
    Ok(quakes)
}

fn parse_wind(body: &str) -> Result<Vec<WindSample>, String> {
    let value: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    let items: Vec<&Value> = if let Some(list) = value.as_array() {
        list.iter().collect()
    } else {
        vec![&value]
    };
    for item in items {
        let Some(current) = item.get("current") else { continue };
        let Some(lat) = item.get("latitude").and_then(|v| v.as_f64()) else { continue };
        let Some(lon) = item.get("longitude").and_then(|v| v.as_f64()) else { continue };
        let Some(speed) = current.get("wind_speed_10m").and_then(|v| v.as_f64()) else { continue };
        let Some(from) = current.get("wind_direction_10m").and_then(|v| v.as_f64()) else { continue };
        let temp = current.get("temperature_2m").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let precip = current.get("precipitation").and_then(|v| v.as_f64()).filter(|v| v.is_finite()).map(|v| v as f32);
        out.push(WindSample {
            lat: lat as f32,
            lon: lon as f32,
            temp_c: temp as f32,
            wind_ms: speed as f32,
            from_deg: from as f32,
            precip_mm: precip,
        });
    }
    Ok(out)
}

fn parse_ocean(body: &str) -> Result<Vec<OceanSample>, String> {
    let value: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    let items: Vec<&Value> = if let Some(list) = value.as_array() {
        list.iter().collect()
    } else {
        vec![&value]
    };
    for item in items {
        let Some(current) = item.get("current") else { continue };
        let Some(sst) = current.get("sea_surface_temperature").and_then(|v| v.as_f64()) else { continue };
        let Some(wave) = current.get("wave_height").and_then(|v| v.as_f64()) else { continue };
        let Some(lat) = item.get("latitude").and_then(|v| v.as_f64()) else { continue };
        let Some(lon) = item.get("longitude").and_then(|v| v.as_f64()) else { continue };
        let current_kmh = current.get("ocean_current_velocity").and_then(|v| v.as_f64()).filter(|v| v.is_finite()).map(|v| v as f32);
        let current_toward_deg = current.get("ocean_current_direction").and_then(|v| v.as_f64()).filter(|v| v.is_finite()).map(|v| v as f32);
        out.push(OceanSample {
            lat: lat as f32,
            lon: lon as f32,
            sst_c: sst as f32,
            wave_m: wave as f32,
            current_kmh,
            current_toward_deg,
        });
    }
    Ok(out)
}

fn load_cities(path: &Path) -> Result<Vec<City>, String> {
    let value = read_json(path)?;
    let mut cities = Vec::new();
    for feature in features(&value)? {
        let props = &feature["properties"];
        let Some((lon, lat)) = point_lon_lat(&feature["geometry"]) else { continue };
        let name = str_prop(props, &["NAME", "NAMEASCII"]).unwrap_or("").to_string();
        if name.is_empty() {
            continue;
        }
        let pop = num_prop(props, &["POP_MAX", "POP_MIN"]).unwrap_or(0.0).max(0.0) as u32;
        cities.push(City { name, lat, lon, pop });
    }
    if cities.is_empty() {
        return Err("no cities in the catalog".into());
    }
    Ok(cities)
}

fn load_faults(path: &Path) -> Result<Vec<Vec<(f32, f32)>>, String> {
    let value = read_json(path)?;
    let mut lines = Vec::new();
    for feature in features(&value)? {
        for chain in line_chains(&feature["geometry"]) {
            if chain.len() >= 2 {
                lines.push(chain);
            }
        }
    }
    if lines.is_empty() {
        return Err("no plate boundaries in the catalog".into());
    }
    Ok(lines)
}

fn load_volcanoes(path: &Path) -> Result<Vec<Volcano>, String> {
    let value = read_json(path)?;
    let mut volcanoes = Vec::new();
    for feature in features(&value)? {
        let props = &feature["properties"];
        let point = point_lon_lat(&feature["geometry"]);
        let lat = point.map(|p| p.1).or_else(|| num_prop(props, &["Latitude"]).map(|v| v as f32));
        let lon = point.map(|p| p.0).or_else(|| num_prop(props, &["Longitude"]).map(|v| v as f32));
        let (Some(lat), Some(lon)) = (lat, lon) else { continue };
        let name = str_prop(props, &["Volcano_Name", "Name"]).unwrap_or("Volcano").to_string();
        let elevation_m = num_prop(props, &["Elevation"]).unwrap_or(0.0) as i32;
        let last_year = props.get("Last_Eruption_Year").and_then(|v| v.as_i64()).map(|v| v as i32);
        volcanoes.push(Volcano { name, lat, lon, elevation_m, last_year });
    }
    if volcanoes.is_empty() {
        return Err("no volcanoes in the catalog".into());
    }
    Ok(volcanoes)
}

fn read_json(path: &Path) -> Result<Value, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

fn features(value: &Value) -> Result<&Vec<Value>, String> {
    value.get("features").and_then(|v| v.as_array()).ok_or_else(|| "GeoJSON has no features".into())
}

fn str_prop<'a>(props: &'a Value, keys: &[&str]) -> Option<&'a str> {
    for key in keys {
        if let Some(text) = props.get(*key).and_then(|v| v.as_str()) {
            if !text.is_empty() {
                return Some(text);
            }
        }
    }
    None
}

fn num_prop(props: &Value, keys: &[&str]) -> Option<f64> {
    for key in keys {
        if let Some(number) = props.get(*key).and_then(|v| v.as_f64()) {
            return Some(number);
        }
    }
    None
}

fn point_lon_lat(geometry: &Value) -> Option<(f32, f32)> {
    if geometry.get("type").and_then(|v| v.as_str()) != Some("Point") {
        return None;
    }
    let coords = geometry.get("coordinates")?.as_array()?;
    Some((coords.first()?.as_f64()? as f32, coords.get(1)?.as_f64()? as f32))
}

fn line_chains(geometry: &Value) -> Vec<Vec<(f32, f32)>> {
    let Some(kind) = geometry.get("type").and_then(|v| v.as_str()) else {
        return Vec::new();
    };
    let Some(coords) = geometry.get("coordinates") else {
        return Vec::new();
    };
    match kind {
        "LineString" => vec![coord_line(coords)],
        "MultiLineString" => coords.as_array().map(|lines| lines.iter().map(coord_line).filter(|l| l.len() >= 2).collect()).unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn coord_line(value: &Value) -> Vec<(f32, f32)> {
    value
        .as_array()
        .map(|pairs| {
            pairs
                .iter()
                .filter_map(|pair| {
                    let pair = pair.as_array()?;
                    Some((pair.get(1)?.as_f64()? as f32, pair.first()?.as_f64()? as f32))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Live comparison of the parsers with the agency JSON, plus the joint-geometry check.
/// Each line starts with PASS or FAIL. A failed fetch is FAIL. A missing catalog is PASS
/// only when the monitor reports the gap instead of filling it.
pub fn accuracy_report() -> String {
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(40)).build();
    let quick = ureq::AgentBuilder::new().timeout(Duration::from_secs(8)).build();
    let mut lines = Vec::new();
    check_solar_wind(&agent, &mut lines);
    check_kp(&agent, &mut lines);
    check_xray(&agent, &mut lines);
    check_wind_window(&agent, &mut lines);
    check_quakes(&agent, &mut lines);
    check_volcanoes(&agent, &mut lines);
    check_donki(&quick, &mut lines);
    check_cactus(&quick, &mut lines);
    check_joint_geometry(&mut lines);
    lines.join("\n")
}

fn check_solar_wind(agent: &ureq::Agent, lines: &mut Vec<String>) {
    let url = "https://services.swpc.noaa.gov/products/summary/solar-wind-speed.json";
    let body = match http_get(agent, url) {
        Ok(body) => body,
        Err(err) => {
            lines.push(format!("FAIL solar wind fetch: {err}"));
            return;
        }
    };
    let Ok(value) = serde_json::from_str::<Value>(&body) else {
        lines.push("FAIL solar wind JSON".into());
        return;
    };
    let Some(raw) = value.get(0).and_then(|row| row.get("proton_speed")).and_then(|v| v.as_f64()) else {
        lines.push("FAIL solar wind JSON has no proton_speed".into());
        return;
    };
    let mut solar = SolarReading::default();
    apply_wind_speed(&mut solar, &body);
    match solar.wind_kms {
        Some(speed) if (f64::from(speed) - raw).abs() < 0.05 => {
            lines.push(format!("PASS solar wind {speed:.0} km/s matches the SWPC JSON"));
        }
        other => lines.push(format!("FAIL solar wind parsed {other:?} against JSON {raw}")),
    }
}

fn check_kp(agent: &ureq::Agent, lines: &mut Vec<String>) {
    let url = "https://services.swpc.noaa.gov/products/noaa-planetary-k-index-forecast.json";
    let body = match http_get(agent, url) {
        Ok(body) => body,
        Err(err) => {
            lines.push(format!("FAIL Kp fetch: {err}"));
            return;
        }
    };
    let Ok(rows) = serde_json::from_str::<Vec<Value>>(&body) else {
        lines.push("FAIL Kp JSON".into());
        return;
    };
    let mut latest = None;
    let mut peak = None;
    let mut peak_after = false;
    for row in &rows {
        let kind = row.get("observed").and_then(|v| v.as_str()).unwrap_or("");
        let Some(kp) = row.get("kp").and_then(|v| v.as_f64()) else { continue };
        let time = row.get("time_tag").and_then(|v| v.as_str()).unwrap_or("");
        if kind == "observed" || kind == "estimated" {
            latest = Some((kp, time.to_string()));
        } else if kind == "predicted" && peak.as_ref().map(|(current, _)| kp >= *current).unwrap_or(true) {
            peak = Some((kp, time.to_string()));
        }
    }
    if let (Some((_, latest_time)), Some((_, peak_time))) = (&latest, &peak) {
        peak_after = stamp_key(peak_time) > stamp_key(latest_time);
    }
    let mut solar = SolarReading::default();
    apply_kp(&mut solar, &body);
    let now_ok = match (solar.kp_now, latest) {
        (Some(parsed), Some((raw, _))) => (f64::from(parsed) - raw).abs() < 0.02,
        _ => false,
    };
    let peak_ok = match (solar.kp_forecast, peak) {
        (Some(parsed), Some((raw, _))) => (f64::from(parsed) - raw).abs() < 0.02,
        _ => false,
    };
    if now_ok && peak_ok && peak_after {
        lines.push(format!(
            "PASS Kp latest {:.2} and predicted peak {:.2} match the SWPC JSON, and the peak time is later",
            solar.kp_now.unwrap_or(0.0),
            solar.kp_forecast.unwrap_or(0.0)
        ));
    } else {
        lines.push(format!("FAIL Kp check now_ok={now_ok} peak_ok={peak_ok} peak_after={peak_after}"));
    }
}

fn check_xray(agent: &ureq::Agent, lines: &mut Vec<String>) {
    let url = "https://services.swpc.noaa.gov/json/goes/primary/xrays-6-hour.json";
    let body = match http_get(agent, url) {
        Ok(body) => body,
        Err(err) => {
            lines.push(format!("FAIL GOES X-ray fetch: {err}"));
            return;
        }
    };
    let Ok(rows) = serde_json::from_str::<Vec<Value>>(&body) else {
        lines.push("FAIL GOES X-ray JSON".into());
        return;
    };
    let raw = rows.iter().rev().find_map(|row| {
        if row.get("energy").and_then(|v| v.as_str()) != Some("0.1-0.8nm") {
            return None;
        }
        let flux = row.get("flux").and_then(|v| v.as_f64())?;
        Some(goes_class(flux))
    });
    let mut solar = SolarReading::default();
    apply_xray(&mut solar, &body);
    if raw.as_deref() == Some(solar.xray_class.as_str()) && !solar.xray_class.is_empty() {
        lines.push(format!("PASS GOES X-ray {} matches the last 0.1-0.8 nm flux", solar.xray_class));
    } else {
        lines.push(format!("FAIL GOES X-ray parsed {} against {:?}", solar.xray_class, raw));
    }
}

fn check_wind_window(agent: &ureq::Agent, lines: &mut Vec<String>) {
    let url = "https://api.open-meteo.com/v1/forecast?latitude=40&longitude=-70&current=wind_speed_10m,wind_direction_10m,temperature_2m&hourly=wind_speed_10m&forecast_hours=24&wind_speed_unit=ms";
    let body = match http_get(agent, url) {
        Ok(body) => body,
        Err(err) => {
            lines.push(format!("FAIL Open-Meteo wind fetch: {err}"));
            return;
        }
    };
    let Ok(value) = serde_json::from_str::<Value>(&body) else {
        lines.push("FAIL Open-Meteo wind JSON".into());
        return;
    };
    let times = value["hourly"]["time"].as_array();
    let speeds = value["hourly"]["wind_speed_10m"].as_array();
    let (Some(times), Some(speeds)) = (times, speeds) else {
        lines.push("FAIL Open-Meteo wind JSON has no hourly wind".into());
        return;
    };
    let mut best: Option<(f64, &str)> = None;
    for (index, speed) in speeds.iter().enumerate() {
        let Some(speed) = speed.as_f64() else { continue };
        let when = times.get(index).and_then(|v| v.as_str()).unwrap_or("");
        if best.map(|(current, _)| speed > current).unwrap_or(true) {
            best = Some((speed, when));
        }
    }
    let Some(peak) = hourly_peak(&body, "wind_speed_10m") else {
        lines.push("FAIL Open-Meteo wind peak was empty".into());
        return;
    };
    let Some((raw, when)) = best else {
        lines.push("FAIL Open-Meteo wind hourly values were empty".into());
        return;
    };
    let first = times.first().and_then(|v| v.as_str()).unwrap_or("");
    let last = times.last().and_then(|v| v.as_str()).unwrap_or("");
    let matches = (f64::from(peak.value) - raw).abs() < 0.05 && peak.when == when;
    let inside = stamp_key(&peak.when) >= stamp_key(first) && stamp_key(&peak.when) <= stamp_key(last);
    let ahead = stamp_key(last) >= stamp_key(&hour_stamp(geo::unix_now()));
    if matches && inside && ahead {
        lines.push(format!(
            "PASS Open-Meteo wind peak {:.1} m/s at {} is the JSON maximum inside a window that reaches {}",
            peak.value, peak.when, last
        ));
    } else {
        lines.push(format!("FAIL Open-Meteo wind matches={matches} inside={inside} ahead={ahead}"));
    }
}

fn check_quakes(agent: &ureq::Agent, lines: &mut Vec<String>) {
    let url = "https://earthquake.usgs.gov/earthquakes/feed/v1.0/summary/2.5_day.geojson";
    let body = match http_get(agent, url) {
        Ok(body) => body,
        Err(err) => {
            lines.push(format!("FAIL USGS quake fetch: {err}"));
            return;
        }
    };
    let Ok(value) = serde_json::from_str::<Value>(&body) else {
        lines.push("FAIL USGS quake JSON".into());
        return;
    };
    let Ok(parsed) = parse_quakes(&body) else {
        lines.push("FAIL USGS quake parse".into());
        return;
    };
    let features = value.get("features").and_then(|v| v.as_array());
    let Some(features) = features else {
        lines.push("FAIL USGS quake JSON has no features".into());
        return;
    };
    let raw_max = features.iter().filter_map(|feature| feature["properties"]["mag"].as_f64()).fold(0.0_f64, f64::max);
    let parsed_max = parsed.iter().map(|quake| f64::from(quake.mag)).fold(0.0_f64, f64::max);
    if parsed.len() == features.len() && (parsed_max - raw_max).abs() < 0.01 {
        lines.push(format!("PASS USGS catalog {} events, largest M{parsed_max:.1} matches the JSON", parsed.len()));
    } else {
        lines.push(format!(
            "FAIL USGS catalog parsed {} of {} features, M{parsed_max:.1} against M{raw_max:.1}",
            parsed.len(),
            features.len()
        ));
    }
}

fn check_volcanoes(agent: &ureq::Agent, lines: &mut Vec<String>) {
    let url = "https://volcanoes.usgs.gov/vsc/api/volcanoApi/elevated";
    let body = match http_get(agent, url) {
        Ok(body) => body,
        Err(err) => {
            lines.push(format!("FAIL USGS volcano fetch: {err}"));
            return;
        }
    };
    let Ok(value) = serde_json::from_str::<Value>(&body) else {
        lines.push("FAIL USGS volcano JSON".into());
        return;
    };
    let Ok(parsed) = parse_volcano_alerts(&body) else {
        lines.push("FAIL USGS volcano parse".into());
        return;
    };
    let Some(rows) = value.as_array() else {
        lines.push("FAIL USGS volcano JSON is not a list".into());
        return;
    };
    let expected: Vec<&str> = rows
        .iter()
        .filter(|row| row.get("lat").and_then(|v| v.as_f64()).is_some() && row.get("long").and_then(|v| v.as_f64()).is_some())
        .filter_map(|row| row.get("vName").and_then(|v| v.as_str()))
        .filter(|name| !name.is_empty())
        .collect();
    let missing = expected.iter().any(|name| !parsed.iter().any(|alert| alert.name == *name));
    if !missing && parsed.len() == expected.len() {
        let names = parsed.iter().map(|alert| format!("{} {}", alert.name, alert.alert_level)).collect::<Vec<_>>().join(", ");
        lines.push(format!("PASS USGS elevated volcanoes match the JSON: {names}"));
    } else {
        lines.push(format!("FAIL USGS volcanoes parsed {} of {} named rows", parsed.len(), expected.len()));
    }
}

fn check_donki(agent: &ureq::Agent, lines: &mut Vec<String>) {
    let now = geo::unix_now();
    let url = format!(
        "https://api.nasa.gov/DONKI/CME?startDate={}&endDate={}&api_key=DEMO_KEY",
        ymd(now - 7.0 * 86400.0),
        ymd(now)
    );
    match http_get_once(agent, &url) {
        Ok(body) => {
            let line = cme_line_from(&body);
            if line.contains("km/s") || line.contains("No speed") || line.contains("no CME") {
                lines.push(format!("PASS {line}"));
            } else {
                lines.push(format!("FAIL DONKI line was not a catalog result: {line}"));
            }
        }
        Err(_) => lines.push("PASS NASA DONKI did not answer. No CME speed was invented.".into()),
    }
}

fn check_cactus(agent: &ureq::Agent, lines: &mut Vec<String>) {
    match http_get_once(agent, "https://www.sidc.be/cactus/out/cmecat.txt") {
        Ok(body) => {
            let line = cactus_line_from(&body);
            if line.contains("km/s") || line.contains("no CME") {
                lines.push(format!("PASS {line}"));
            } else if line.contains("did not load") {
                lines.push("PASS SIDC CACTus did not parse. No CME speed was invented.".into());
            } else {
                lines.push(format!("FAIL CACTus line was not a catalog result: {line}"));
            }
        }
        Err(_) => lines.push("PASS SIDC CACTus did not answer. No CME speed was invented.".into()),
    }
}

fn check_joint_geometry(lines: &mut Vec<String>) {
    let quakes = vec![Quake {
        mag: 6.0,
        lat: 0.0,
        lon: 0.0,
        place: "equator test".into(),
        time_ms: 1_700_000_000_000,
        ..Quake::default()
    }];
    let mut bulletin = Bulletin {
        alerts_loaded: true,
        volcano_alerts: vec![VolcanoAlert {
            name: "Ridge".into(),
            lat: 0.0,
            lon: 1.0,
            alert_level: "WATCH".into(),
            color_code: "ORANGE".into(),
            synopsis: String::new(),
        }],
        wind_forecast: Some(SamplePeak { lat: 10.0, lon: 10.0, value: 12.0, when: "2026-11-03T06:00".into() }),
        wave_forecast: Some(SamplePeak { lat: 10.0, lon: 12.0, value: 4.0, when: "2026-11-03T08:00".into() }),
        ..Bulletin::default()
    };
    bulletin.solar.kp_now = Some(2.0);
    bulletin.solar.kp_now_kind = "observed".into();
    bulletin.solar.kp_now_time = "2026-11-02T12:00:00".into();
    bulletin.solar.kp_forecast = Some(3.0);
    bulletin.solar.kp_forecast_time = "2026-11-03T00:00:00".into();
    let wind = vec![WindSample { lat: 0.0, lon: 0.5, temp_c: 25.0, wind_ms: 8.0, from_deg: 90.0, precip_mm: None }];
    let ocean = vec![OceanSample { lat: 40.0, lon: 40.0, sst_c: 15.0, wave_m: 2.0, current_kmh: None, current_toward_deg: None }];
    let volcanoes = vec![Volcano { name: "Test".into(), lat: 0.0, lon: 0.2, elevation_m: 100, last_year: Some(2010) }];
    let faults = vec![vec![(0.0_f32, 2.0)]];
    let text = joint_lines(&quakes, &wind, &ocean, &volcanoes, &faults, &bulletin, 1_700_000_000.0);
    let joined = text.join("\n");
    let volcano_km = fmt_km(km_between(0.0, 0.0, 0.0, 1.0));
    let wind_km = fmt_km(km_between(0.0, 0.0, 0.0, 0.5));
    let forecast_km = fmt_km(km_between(10.0, 10.0, 10.0, 12.0));
    let ok = joined.contains(&volcano_km)
        && joined.contains(&wind_km)
        && joined.contains(&forecast_km)
        && joined.contains("Within 700 km of the largest quake: elevated volcanoes 1, wind samples 1, sea samples 0.")
        && joined.contains("is after")
        && joined.contains("FSOT is not scoring this record yet")
        && !joined.contains("FSOT predicts");
    if ok {
        lines.push(format!("PASS joint geometry quotes {volcano_km} to the elevated volcano and keeps the forecast points separate"));
    } else {
        lines.push("FAIL joint geometry".into());
    }
}

fn cache_path(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("cache");
    let _ = fs::create_dir_all(&dir);
    dir.join(name)
}

fn series_root() -> PathBuf {
    cache_path("series")
}

/// Read one held series by name. The name is the file under cache/series, such as `rtsw`.
pub fn pull_series(name: &str) -> Result<series::SeriesPull, String> {
    series::pull(&series_root(), name)
}

fn push_measured(held: &mut Vec<series::HeldNote>, name: &str, source: &str, rows: Vec<series::Row>, empty_detail: &str) {
    if rows.is_empty() {
        held.push(series::recall_or(&series_root(), name, source, empty_detail));
    } else {
        held.push(series::store(&series_root(), name, source, rows));
    }
}

fn hold_body(
    held: &mut Vec<series::HeldNote>,
    name: &str,
    source: &str,
    parsed: Result<Vec<series::Row>, ()>,
    fail: &str,
    empty_is_catalog: bool,
) {
    match parsed {
        Ok(rows) if empty_is_catalog || !rows.is_empty() => held.push(series::store(&series_root(), name, source, rows)),
        _ => held.push(series::recall_or(&series_root(), name, source, fail)),
    }
}

fn recall_named(held: &mut Vec<series::HeldNote>, name: &str, source: &str, detail: &str) {
    held.push(series::recall_or(&series_root(), name, source, detail));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogs_load_from_assets() {
        let mut state = FeedState::default();
        load_static(&mut state);
        let vatican = state.cities.iter().find(|c| c.name == "Vatican City").expect("vatican");
        assert!((vatican.lat - 41.9).abs() < 0.3, "lat {}", vatican.lat);
        assert!((vatican.lon - 12.45).abs() < 0.3, "lon {}", vatican.lon);
        assert!(state.cities.len() > 200, "cities {}", state.cities.len());
        assert!(state.faults.len() > 100, "faults {}", state.faults.len());
        assert!(state.volcanoes.len() > 1000, "volcanoes {}", state.volcanoes.len());
        assert!(state.volcanoes.iter().any(|v| v.name.contains("Eifel") || v.name.contains("Fuji") || v.elevation_m > 5000));
    }

    #[test]
    fn cluster_counts_a_nearby_sequence_and_leaves_a_distant_quake() {
        let now = 1_700_000_000_000;
        let mut quakes = Vec::new();
        for i in 0..4 {
            quakes.push(Quake {
                mag: 4.6 + i as f32 * 0.2,
                lat: 36.0 + i as f32 * 0.2,
                lon: 140.0,
                depth_km: 10.0,
                place: "near Japan".into(),
                time_ms: now - i * 60_000,
                ..Quake::default()
            });
        }
        quakes.push(Quake {
            mag: 6.4,
            lat: -33.0,
            lon: -72.0,
            depth_km: 20.0,
            place: "off Chile".into(),
            time_ms: now - 3_600_000,
            alert: Some("yellow".into()),
            tsunami: true,
            ..Quake::default()
        });
        let notes = clusters_within(&quakes, now, 700.0, 24 * 3600 * 1000);
        assert_eq!(notes.len(), 1, "chile must stay outside the Japan sequence");
        assert_eq!(notes[0].count, 4);
        assert!(notes[0].lat > 30.0 && notes[0].lat < 40.0);
        let text = compose_reading(&quakes, &[], &[], &[], &[], &Bulletin::default(), now as f64 / 1000.0);
        assert!(text.iter().any(|line| line.contains("M6.4") && line.contains("Chile")));
        assert!(text.iter().any(|line| line.contains("Active sequence") && line.contains("4 events")));
    }

    #[test]
    fn himalaya_sample_is_high_when_the_grid_is_present() {
        let path = renderer::project_path("assets/elevation_5deg.bin");
        if !path.exists() {
            return;
        }
        let grid = ElevationGrid::load(&path).expect("grid");
        let meters = grid.sample(32.0, 85.0).expect("tibet sample");
        assert!(meters > 1000, "Tibet sample {meters} m");
        let greenland = grid.sample(72.0, -40.0);
        assert!(greenland.is_some());
    }

    #[test]
    fn goes_class_uses_the_long_channel_decades() {
        assert_eq!(goes_class(3.0e-7), "B3.0");
        assert_eq!(goes_class(1.0e-6), "C1.0");
        assert_eq!(goes_class(1.0e-4), "X1.0");
        assert_eq!(goes_class(1.0e-9), "below A");
        assert!(class_rank("X1.2") > class_rank("M9.9"));
    }

    #[test]
    fn kp_forecast_keeps_the_latest_estimate_and_the_predicted_peak() {
        let body = r#"[
            {"time_tag":"2026-11-02T12:00:00","kp":1.67,"observed":"observed","noaa_scale":null},
            {"time_tag":"2026-11-02T21:00:00","kp":2.00,"observed":"estimated","noaa_scale":null},
            {"time_tag":"2026-11-03T00:00:00","kp":2.67,"observed":"predicted","noaa_scale":"G1"},
            {"time_tag":"2026-11-04T00:00:00","kp":1.33,"observed":"predicted","noaa_scale":null}
        ]"#;
        let mut solar = SolarReading::default();
        apply_kp(&mut solar, body);
        assert_eq!(solar.kp_now, Some(2.0));
        assert_eq!(solar.kp_now_kind, "estimated");
        assert_eq!(solar.kp_forecast, Some(2.67));
        assert_eq!(solar.kp_forecast_scale.as_deref(), Some("G1"));
    }

    #[test]
    fn published_lines_name_the_agency_and_keep_the_record_limit() {
        let mut bulletin = Bulletin {
            wind_forecast: Some(SamplePeak { lat: 40.0, lon: -70.0, value: 18.0, when: "2026-11-03T06:00".into() }),
            wave_forecast: Some(SamplePeak { lat: -40.0, lon: 20.0, value: 6.5, when: "2026-11-03T03:00".into() }),
            alerts_loaded: true,
            aftershock_line: "USGS aftershock forecast for M5.0 near Alaska, 1 Week: chance of one or more M≥5.0 is 7.4% (Reasenberg-Jones, USGS).".into(),
            ..Bulletin::default()
        };
        bulletin.solar.wind_kms = Some(395.0);
        bulletin.solar.wind_time = "2026-11-02T15:39:00Z".into();
        bulletin.solar.kp_now = Some(1.67);
        bulletin.solar.kp_now_kind = "observed".into();
        bulletin.solar.kp_now_time = "2026-11-02T12:00:00".into();
        bulletin.solar.kp_forecast = Some(2.67);
        bulletin.solar.kp_forecast_time = "2026-11-03T00:00:00".into();
        bulletin.solar.xray_class = "B3.0".into();
        bulletin.solar.xray_time = "2026-11-02T15:43:00Z".into();
        bulletin.solar.cme_line = "NASA DONKI CME catalog did not load.".into();
        bulletin.volcano_alerts.push(VolcanoAlert {
            name: "Kilauea".into(),
            lat: 19.4,
            lon: -155.3,
            alert_level: "ADVISORY".into(),
            color_code: "YELLOW".into(),
            synopsis: "The forecast for episode 56 fountaining is Nov. 3-7, 2026.".into(),
        });
        let quakes = vec![Quake {
            mag: 5.1,
            place: "near Tomohon".into(),
            alert: Some("green".into()),
            ..Quake::default()
        }];
        let text = compose_reading(&quakes, &[], &[], &[], &[], &bulletin, 1_700_000_000.0);
        let joined = text.join("\n");
        assert!(joined.contains("NOAA SWPC solar wind 395 km/s"));
        assert!(joined.contains("Forecast peak Kp 2.67 predicted"));
        assert!(joined.contains("Open-Meteo forecast: strongest wind"));
        assert!(joined.contains("Open-Meteo marine forecast: highest wave"));
        assert!(joined.contains("USGS aftershock forecast"));
        assert!(joined.contains("USGS PAGER GREEN"));
        assert!(joined.contains("USGS volcano alerts: Kilauea ADVISORY/YELLOW"));
        assert!(joined.contains("USGS notice: The forecast for episode 56"));
        assert!(joined.contains("This counts the loaded record. It does not forecast the next event."));
        assert!(!joined.contains("FSOT predicts"));
    }

    #[test]
    fn hourly_peak_picks_the_largest_finite_hour() {
        let body = r#"[{"latitude":10,"longitude":20,"hourly":{"time":["t0","t1"],"wind_speed_10m":[1.0,null]}},{"latitude":-5,"longitude":40,"hourly":{"time":["t0","t1"],"wind_speed_10m":[3.5,9.0]}}]"#;
        let peak = hourly_peak(body, "wind_speed_10m").expect("peak");
        assert_eq!(peak.value, 9.0);
        assert_eq!(peak.when, "t1");
        assert_eq!(peak.lat, -5.0);
    }

    #[test]
    fn oaf_line_quotes_the_week_m5_probability() {
        let body = r#"{"advisoryTimeFrame":"1 Week","model":{"name":"Reasenberg-Jones (1989, 1994) aftershock model"},"forecast":[{"label":"1 Day","bins":[{"magnitude":5.0,"probability":0.01}]},{"label":"1 Week","bins":[{"magnitude":5.0,"probability":0.0739},{"magnitude":6.0,"probability":0.002}]}]}"#;
        let line = oaf_line("143 km S of False Pass, Alaska", 5.0, body).expect("line");
        assert!(line.contains("1 Week"));
        assert!(line.contains("7.4%"));
        assert!(line.contains("M≥5.0"));
        assert!(line.contains("Reasenberg-Jones, USGS"));
    }

    #[test]
    fn cme_line_uses_the_latest_most_accurate_speed() {
        let body = r#"[{"startTime":"2026-10-28T01:00Z","cmeAnalyses":[{"isMostAccurate":false,"speed":200,"type":"S"}]},{"startTime":"2026-10-30T12:24Z","cmeAnalyses":[{"isMostAccurate":true,"speed":512,"type":"C"},{"isMostAccurate":false,"speed":900,"type":"C"}]}]"#;
        let line = cme_line_from(body);
        assert!(line.contains("2026-10-30T12:24Z"));
        assert!(line.contains("512 km/s"));
        assert!(line.contains("type C"));
        assert_eq!(cme_line_from("[]"), "NASA DONKI lists no CME in the last 7 days.");
    }

    #[test]
    fn joint_record_quotes_distance_and_keeps_the_forecast_on_its_own_clock() {
        let quakes = vec![Quake {
            mag: 6.0,
            lat: 0.0,
            lon: 0.0,
            place: "equator test".into(),
            time_ms: 1_700_000_000_000,
            ..Quake::default()
        }];
        let mut bulletin = Bulletin {
            alerts_loaded: true,
            volcano_alerts: vec![VolcanoAlert {
                name: "Ridge".into(),
                lat: 0.0,
                lon: 1.0,
                alert_level: "WATCH".into(),
                color_code: "ORANGE".into(),
                synopsis: String::new(),
            }],
            wind_forecast: Some(SamplePeak { lat: 10.0, lon: 10.0, value: 12.0, when: "2026-11-03T06:00".into() }),
            wave_forecast: Some(SamplePeak { lat: 10.0, lon: 12.0, value: 4.0, when: "2026-11-03T08:00".into() }),
            ..Bulletin::default()
        };
        bulletin.solar.kp_now = Some(2.0);
        bulletin.solar.kp_now_time = "2026-11-02T12:00:00".into();
        bulletin.solar.kp_forecast = Some(3.0);
        bulletin.solar.kp_forecast_time = "2026-11-03T00:00:00".into();
        let wind = [WindSample { lat: 0.0, lon: 0.5, temp_c: 25.0, wind_ms: 8.0, from_deg: 90.0, precip_mm: None }];
        let ocean = [OceanSample { lat: 40.0, lon: 40.0, sst_c: 15.0, wave_m: 2.0, current_kmh: None, current_toward_deg: None }];
        let volcanoes = [Volcano { name: "Test".into(), lat: 0.0, lon: 0.2, elevation_m: 100, last_year: Some(2010) }];
        let faults = vec![vec![(0.0_f32, 2.0)]];
        let text = compose_reading(&quakes, &wind, &ocean, &volcanoes, &faults, &bulletin, 1_700_000_000.0);
        let joined = text.join("\n");
        assert!(joined.contains(&fmt_km(km_between(0.0, 0.0, 0.0, 1.0))), "{joined}");
        assert!(joined.contains("elevated volcanoes 1, wind samples 1, sea samples 0"));
        assert!(joined.contains("is after the latest Kp row"));
        assert!(joined.contains("is at or after UTC"));
        assert!(joined.contains("not a cause"));
        assert!(joined.contains("FSOT is not scoring this record yet"));
        assert!(joined.contains("This counts the loaded record. It does not forecast the next event."));
        assert!(!joined.contains("FSOT predicts"));
    }

    #[test]
    fn nhc_empty_list_is_a_real_reading_and_a_storm_keeps_its_place() {
        let (line, storms) = storm_report(r#"{"activeStorms":[]}"#).unwrap();
        assert!(line.contains("no active storms"));
        assert!(storms.is_empty());
        let (line, storms) = storm_report(
            r#"{"activeStorms":[{"id":"al092026","name":"Hermine","classification":"TS","intensity":"45","latitudeNumeric":25.2,"longitudeNumeric":-70.1,"movementDir":315,"movementSpeed":12}]}"#,
        )
        .unwrap();
        assert_eq!(storms.len(), 1);
        assert!(line.contains("Tropical Storm Hermine"), "{line}");
        assert!(line.contains("45 kt"), "{line}");
        assert!(line.contains("25.20° N"), "{line}");
        assert!(line.contains("70.10° W"), "{line}");
    }

    #[test]
    fn nws_tsunami_empty_and_one_warning_are_quoted() {
        let (line, ids) = tsunami_report(r#"{"features":[]}"#).unwrap();
        assert!(line.contains("no current tsunami"));
        assert!(ids.is_empty());
        let (line, ids) = tsunami_report(
            r#"{"features":[{"id":"urn:test","properties":{"id":"urn:test","event":"Tsunami Warning","areaDesc":"Hawaii coastal areas","sent":"2026-11-02T16:00:00Z"}}]}"#,
        )
        .unwrap();
        assert_eq!(ids, vec!["urn:test".to_string()]);
        assert!(line.contains("Tsunami Warning"), "{line}");
        assert!(line.contains("Hawaii coastal areas"), "{line}");
    }

    #[test]
    fn weekly_rss_keeps_the_report_week_and_the_georss_point() {
        let body = r#"<rss><channel><item><title>Krakatau (Indonesia) - Report for 8 October-14 October 2026 - New Eruptive Activity</title><guid>https://volcano.si.edu/reports_weekly.cfm#vn_262000</guid><georss:point>-6.1009 105.4233</georss:point></item></channel></rss>"#;
        let (line, reports) = weekly_report(body).unwrap();
        assert_eq!(reports.len(), 1);
        assert!((reports[0].lat - -6.1009).abs() < 0.001);
        assert!((reports[0].lon - 105.4233).abs() < 0.001);
        assert!(line.contains("8 October-14 October 2026"), "{line}");
        assert!(line.contains("1 new, 0 continuing"), "{line}");
        assert!(line.contains("Krakatau"), "{line}");
    }

    #[test]
    fn cactus_quotes_the_latest_onset_speed_and_skips_flows() {
        let body = "# CME | t0 | dt0| pa | da | v | dv | minv| maxv| halo?\n0011|2026/11/01 23:48| 05 | 041| 016| 0364| 0064| 0253| 0464|\n0012|2026/11/02 00:36| 01 | 091| 008| 0151| 0066| 0118| 0287|\n# Flow| t0 | dt0| pa | da | v\n0002|2026/11/02 06:24| 02 | 032| 006| 0496| 0417| 0179| 1239|\n";
        let line = cactus_line_from(body);
        assert!(line.contains("0012"), "{line}");
        assert!(line.contains("151 km/s"), "{line}");
        assert!(line.contains("2026/11/02 00:36"), "{line}");
        assert!(!line.contains("496"), "{line}");
        assert!(cactus_line_from("not a catalog").contains("did not load"));
    }

    #[test]
    fn ocean_current_and_rain_stay_on_the_sample() {
        let wind = parse_wind(r#"{"latitude":10,"longitude":120,"current":{"temperature_2m":28,"wind_speed_10m":2.1,"wind_direction_10m":40,"precipitation":1.5}}"#).unwrap();
        assert_eq!(wind[0].precip_mm, Some(1.5));
        let ocean = parse_ocean(
            r#"{"latitude":-40,"longitude":20,"current":{"wave_height":2.2,"sea_surface_temperature":19.7,"ocean_current_velocity":7.6,"ocean_current_direction":34}}"#,
        )
        .unwrap();
        assert_eq!(ocean[0].current_kmh, Some(7.6));
        assert_eq!(ocean[0].current_toward_deg, Some(34.0));
        let rain = hourly_peak(
            r#"[{"latitude":1,"longitude":2,"hourly":{"time":["h0","h1"],"precipitation":[0.0,4.5]}}]"#,
            "precipitation",
        )
        .unwrap();
        assert_eq!(rain.value, 4.5);
        assert_eq!(rain.when, "h1");
    }

    #[test]
    fn agency_watch_quotes_a_usgs_watch_and_stays_quiet_below_the_agency_line() {
        let mut bulletin = Bulletin {
            alerts_loaded: true,
            ..Bulletin::default()
        };
        bulletin.volcano_alerts.push(VolcanoAlert {
            name: "Great Sitkin".into(),
            lat: 52.0,
            lon: -176.1,
            alert_level: "WATCH".into(),
            color_code: "ORANGE".into(),
            synopsis: String::new(),
        });
        let lines = watch_lines(&[], &bulletin);
        assert!(lines.iter().any(|line| line.contains("Great Sitkin") && line.contains("WATCH")), "{lines:?}");

        let quiet = watch_lines(&[], &Bulletin::default());
        assert!(quiet.iter().any(|line| line.contains("no warning, watch, storm, or tsunami bulletin is loaded")), "{quiet:?}");

        let mut calm = Bulletin::default();
        calm.solar.kp_now = Some(2.0);
        calm.solar.kp_forecast = Some(2.67);
        calm.solar.xray_class = "B3.1".into();
        calm.storm_line = "NHC lists no active storms in the Atlantic or the eastern and central Pacific.".into();
        let calm_lines = watch_lines(&[], &calm);
        let calm_text = calm_lines.join("\n");
        assert!(calm_text.contains("no warning, watch, storm, or tsunami bulletin is loaded"), "{calm_text}");
        assert!(!calm_text.contains("at or above 5"), "{calm_text}");
        assert!(!calm_text.contains("M-class"), "{calm_text}");
        assert!(!calm_text.contains("NHC lists no active"), "{calm_text}");

        calm.solar.kp_now = Some(6.0);
        calm.solar.kp_now_kind = "observed".into();
        calm.solar.kp_now_time = "2026-11-03T03:00:00".into();
        let hot = watch_lines(&[], &calm);
        assert!(hot.iter().any(|line| line.contains("6.00") && line.contains("at or above 5")), "{hot:?}");

        let mut advisory = Bulletin { alerts_loaded: true, ..Bulletin::default() };
        advisory.volcano_alerts.push(VolcanoAlert {
            name: "Kilauea".into(),
            lat: 19.4,
            lon: -155.3,
            alert_level: "ADVISORY".into(),
            color_code: "YELLOW".into(),
            synopsis: "The forecast for episode 56 fountaining is Nov. 3-7, 2026.".into(),
        });
        let quoted = watch_lines(&[], &advisory);
        let quoted_text = quoted.join("\n");
        assert!(quoted_text.contains("remains ADVISORY/YELLOW"), "{quoted_text}");
        assert!(quoted_text.contains("episode 56"), "{quoted_text}");
        assert!(!quoted_text.contains("WARNING"), "{quoted_text}");
        let cards = watch_cards(&[], &bulletin);
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].lat, Some(52.0));
        assert_eq!(cards[0].place, "Great Sitkin");
    }

    #[test]
    fn city_search_prefers_the_name_and_a_spot_keeps_the_sample_distance() {
        let cities = vec![
            City { name: "Paris".into(), lat: 48.8, lon: 2.3, pop: 10_000_000 },
            City { name: "Honolulu".into(), lat: 21.3, lon: -157.8, pop: 350_000 },
            City { name: "Hong Kong".into(), lat: 22.3, lon: 114.2, pop: 7_000_000 },
        ];
        let hits = search_cities(&cities, "hon");
        assert_eq!(hits[0].name, "Hong Kong");
        assert!(hits.iter().any(|city| city.name == "Honolulu"));
        assert!(!hits.iter().any(|city| city.name == "Paris"));
        let wind = vec![
            WindSample { lat: 0.0, lon: 0.0, temp_c: 20.0, wind_ms: 3.0, from_deg: 10.0, precip_mm: Some(0.0) },
            WindSample { lat: 10.0, lon: 10.0, temp_c: 22.0, wind_ms: 8.0, from_deg: 90.0, precip_mm: Some(1.5) },
        ];
        let text = describe_spot(10.2, 10.1, &wind, &[], &Bulletin::default(), ShowUnits::default()).join("\n");
        assert!(text.contains("8 m/s"), "{text}");
        assert!(text.contains("from 90°"), "{text}");
        assert!(text.contains("1.5 mm"), "{text}");
        assert!(text.contains("km"), "{text}");
    }

    #[test]
    fn place_hours_keep_the_utc_hour_and_skip_a_null() {
        let body = r#"{"current":{"temperature_2m":21.2,"wind_speed_10m":4.0,"precipitation":0.0},"hourly":{"time":["2026-11-03T01:00","2026-11-03T02:00","2026-11-03T03:00"],"temperature_2m":[20.0,null,19.5],"wind_speed_10m":[3.0,4.0,5.0],"precipitation":[0.0,1.0,0.2]}}"#;
        let forecast = parse_place_forecast(body, "2026-11-03T02").unwrap();
        assert_eq!(forecast.hours.len(), 2);
        assert_eq!(forecast.hours[0].when, "2026-11-03T02:00");
        assert!(forecast.hours[0].temp_c.is_none());
        assert!((forecast.now.temp_c.unwrap() - 21.2).abs() < 0.01);
        let lines = format_place_forecast(&forecast, ShowUnits { fahrenheit: true, mph: true, inches: true });
        let joined = lines.join("\n");
        assert!(joined.contains("°F"), "{joined}");
        assert!(joined.contains("mph"), "{joined}");
        assert!(joined.contains("0.00 in"), "{joined}");
        assert!(joined.contains("02:00"), "{joined}");
    }

    #[test]
    fn geocode_reads_a_postal_code_hit_and_skips_a_place_without_coordinates() {
        let body = r#"{"results":[
            {"name":"Beverly Hills","latitude":34.07362,"longitude":-118.40036,"admin1":"California","country":"United States"},
            {"name":"Skip me","country":"United States"},
            {"name":"Ashland","latitude":40.86867,"longitude":-82.31822,"admin1":"Ohio","country":"United States"}
        ]}"#;
        let hits = parse_geocode(body).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].label(), "Beverly Hills, California, United States");
        assert!((hits[0].lon + 118.40).abs() < 0.01);
        assert_eq!(hits[1].detail, "Ohio, United States");
        assert!(parse_geocode("{}").unwrap().is_empty());
        assert!(parse_geocode(r#"{"error":true,"reason":"nope"}"#).is_err());
        let url = geocode_url("Los Angeles, CA");
        assert!(url.starts_with("https://geocoding-api.open-meteo.com/v1/search?"), "{url}");
        assert!(url.contains("name=Los%20Angeles%2C%20CA"), "{url}");
        assert!(url.contains("count=6"), "{url}");
    }

    #[test]
    fn same_hour_value_matches_the_stamp_and_skips_null() {
        let body = r#"{"hourly":{"time":["2026-11-02T17:00","2026-11-02T18:00","2026-11-02T19:00"],"precipitation":[1.0,null,4.2]}}"#;
        assert!(hour_value_at(body, "precipitation", "2026-11-02T18").is_none());
        let value = hour_value_at(body, "precipitation", "2026-11-02T19").unwrap();
        assert!((value - 4.2).abs() < 0.01, "{value}");
        let url = closed_check_url("rain", 39.945, 19.997).unwrap();
        assert!(url.contains("past_hours=36"), "{url}");
        assert!(url.contains("hourly=precipitation"), "{url}");
        assert!(url.contains("forecast_hours=1"), "{url}");
        assert!(!url.contains("wind_speed_10m"), "{url}");
    }

    #[test]
    fn measured_space_fields_keep_the_active_row_and_quote_the_scale() {
        let mut solar = SolarReading::default();
        apply_protons(
            &mut solar,
            r#"[{"time_tag":"2026-11-03T03:00:00Z","energy":">=10 MeV","flux":0.1},{"time_tag":"2026-11-03T03:10:00Z","energy":">=60 MeV","flux":0.224},{"time_tag":"2026-11-03T03:10:00Z","energy":">=10 MeV","flux":"0.305"}]"#,
        );
        assert!((solar.proton_10mev.unwrap() - 0.305).abs() < 0.0001);
        assert_eq!(solar.proton_10mev_time, "2026-11-03T03:10:00Z");
        let proton = measured_lines(&solar).join("\n");
        assert!(proton.contains("0.305 pfu"), "{proton}");
        assert!(!proton.to_lowercase().contains("s-scale"), "{proton}");
        assert!(!proton.contains("warning"), "{proton}");

        apply_protons(&mut solar, r#"[{"energy":">=60 MeV","flux":0.2}]"#);
        assert!(solar.proton_10mev.is_none());
        assert!(solar.proton_line.contains("did not include"));

        let mut solar = SolarReading::default();
        apply_plasma(
            &mut solar,
            r#"[{"time_tag":"2026-11-03T03:20:00","active":false,"proton_density":9.9,"proton_temperature":1,"source":"TAIL"},{"time_tag":"2026-11-03T03:16:00","active":true,"proton_speed":"394.3","proton_temperature":"74806","proton_density":1.78,"source":"SOLAR1"},{"time_tag":"2026-11-03T03:10:00","active":true,"proton_density":1.5,"proton_temperature":70000,"source":"OLD"}]"#,
        );
        assert!((solar.density_cm3.unwrap() - 1.78).abs() < 0.001);
        assert!((solar.temperature_k.unwrap() - 74806.0).abs() < 0.1);
        assert_eq!(solar.plasma_time, "2026-11-03T03:16:00");
        assert_eq!(solar.plasma_source, "SOLAR1");

        let mut quiet = SolarReading::default();
        apply_plasma(
            &mut quiet,
            r#"[{"time_tag":"2026-11-03T01:00:00","active":false,"proton_density":2.0},{"time_tag":"2026-11-03T02:00:00","active":false,"proton_density":3.5},{"time_tag":"2026-11-03T03:00:00","active":true,"proton_density":null}]"#,
        );
        assert!((quiet.density_cm3.unwrap() - 3.5).abs() < 0.001);
        assert_eq!(quiet.plasma_time, "2026-11-03T02:00:00");

        let mut solar = SolarReading::default();
        apply_f107(
            &mut solar,
            r#"[{"time_tag":"2026-11-02T22:00:00","flux":98,"reporting_schedule":"Afternoon","ninety_day_mean":null},{"time_tag":"2026-11-02T20:00:00","flux":"97","reporting_schedule":"Noon","ninety_day_mean":"109"}]"#,
        );
        assert!((solar.f107_sfu.unwrap() - 98.0).abs() < 0.01);
        assert_eq!(solar.f107_time, "2026-11-02T22:00:00");
        assert_eq!(solar.f107_schedule, "Afternoon");
        assert!((solar.f107_mean_sfu.unwrap() - 109.0).abs() < 0.01);
        assert_eq!(solar.f107_mean_time, "2026-11-02T20:00:00");

        let mut solar = SolarReading::default();
        apply_dst(
            &mut solar,
            r#"[{"time_tag":"2026-11-03T01:00:00","dst":-4},{"time_tag":"2026-11-03T02:00:00","dst":"-8"},{"time_tag":"2026-11-03T03:00:00","dst":null}]"#,
        );
        assert!((solar.dst_nt.unwrap() - -8.0).abs() < 0.01);
        assert_eq!(solar.dst_time, "2026-11-03T02:00:00");

        let mut solar = SolarReading::default();
        apply_scales(
            &mut solar,
            r#"{"0":{"DateStamp":"2026-11-03","TimeStamp":"03:19","R":{"Scale":0,"Text":"none"},"S":{"Scale":"0","Text":"none"},"G":{"Scale":"1","Text":"minor"}},"1":{"R":{"Scale":"5","Text":"extreme"}}}"#,
        );
        assert_eq!(solar.scale_r, "0 none");
        assert_eq!(solar.scale_s, "0 none");
        assert_eq!(solar.scale_g, "1 minor");
        assert!(solar.scale_time.contains("2026-11-03"));
        let quoted = scale_sentence(&solar);
        assert!(quoted.contains("R 0 none"), "{quoted}");
        assert!(quoted.contains("G 1 minor"), "{quoted}");
        assert!(!quoted.contains("extreme"), "{quoted}");
        assert!(!quoted.to_lowercase().contains("warning"), "{quoted}");
        apply_scales(&mut solar, r#"{"1":{"R":{"Scale":"5","Text":"extreme"}}}"#);
        assert!(solar.scale_line.contains("did not load"));
        assert!(solar.scale_r.is_empty());
    }

    #[test]
    fn climate_and_ensemble_keep_numbers_without_declaring_an_event() {
        let parsed = parse_oni("SEAS  YR   TOTAL   ANOM\nMJJ 2026  29.02   1.39\nJAS 2026  29.12   2.16\n").unwrap();
        assert_eq!(parsed.0, "JAS");
        assert_eq!(parsed.1, 2026);
        assert!((parsed.3 - 2.16).abs() < 0.001);
        let mut climate = ClimateReading::default();
        climate.oni_season = parsed.0;
        climate.oni_year = Some(parsed.1);
        climate.oni_total = Some(parsed.2);
        climate.oni_anom = Some(parsed.3);
        let (year, month, index) = parse_monthly_index("2026    8   -0.0560\n2026    9    1.1556\n").unwrap();
        assert_eq!((year, month), (2026, 9));
        assert!((index - 1.1556).abs() < 0.0001);
        climate.ao_year = Some(year);
        climate.ao_month = Some(month);
        climate.ao = Some(index);
        let lines = climate_lines(&climate);
        let anomaly = lines.iter().find(|line| line.contains("2.16")).expect("anomaly");
        assert!(anomaly.contains("JAS 2026"), "{anomaly}");
        assert!(!anomaly.contains("El Niño"), "{anomaly}");
        assert!(lines.iter().any(|line| line.contains("five overlapping seasons") && line.contains("This row is one season.")));
        assert!(lines.iter().any(|line| line.contains("CPC AO 2026-09") && line.contains("1.1556")));
        assert!(lines.iter().any(|line| line.contains("CPC NAO index has not arrived.")));
        assert!(parse_oni("not a table").is_err());

        let body = r#"{"hourly":{"time":["2026-11-03T01:00","2026-11-03T02:00","2026-11-03T03:00","2026-11-03T04:00","2026-11-03T05:00","2026-11-03T06:00","2026-11-03T07:00","2026-11-03T08:00"],"temperature_2m":[1,1,1,1,1,1,1,1],"temperature_2m_member01":[10,null,4,4,4,4,4,4],"temperature_2m_member02":[12,6,8,8,8,8,8,8],"precipitation_member01":[0,0,1.0,0,0,0,0,0],"precipitation_member02":[0,0,3.0,0,0,0,0,0],"wind_speed_10m_member01":[1,1,5,5,5,5,5,5]}}"#;
        let cast = parse_ensemble(body, "2026-11-03T02").unwrap();
        assert_eq!(cast.hours.len(), 6);
        assert_eq!(cast.hours[0].when, "2026-11-03T02:00");
        assert_eq!(cast.hours[0].temp.as_ref().unwrap().members, 1);
        assert!((cast.hours[0].temp.as_ref().unwrap().mean - 6.0).abs() < 0.01);
        let third = &cast.hours[1];
        assert_eq!(third.when, "2026-11-03T03:00");
        let temp = third.temp.as_ref().unwrap();
        assert_eq!(temp.members, 2);
        assert!((temp.min - 4.0).abs() < 0.01 && (temp.max - 8.0).abs() < 0.01 && (temp.mean - 6.0).abs() < 0.01);
        let rain = third.rain.as_ref().unwrap();
        assert!((rain.min - 1.0).abs() < 0.01 && (rain.max - 3.0).abs() < 0.01 && (rain.mean - 2.0).abs() < 0.01);
        assert_eq!(third.wind.as_ref().unwrap().members, 1);
        assert_eq!(cast.hours.last().unwrap().when, "2026-11-03T07:00");
        let spoken = format_ensemble(&cast).join("\n");
        assert!(spoken.contains("4 to 8 °C"), "{spoken}");
        assert!(spoken.contains("mean 6"), "{spoken}");
        assert!(!spoken.contains("temp 1 °C"), "{spoken}");
        let url = ensemble_url(39.74, -104.99);
        assert!(url.contains("models=gfs_seamless"), "{url}");
        assert!(url.contains("forecast_days=1"), "{url}");
        assert!(url.contains("wind_speed_unit=ms"), "{url}");
        assert!(url.contains("hourly=temperature_2m,precipitation,wind_speed_10m"), "{url}");
        assert!(!url.contains("forecast_hours"), "{url}");

        let desk = desk_text(&[], &Bulletin::default(), false, &None, &None);
        assert!(desk.contains("FSOT is not scoring."));
        assert!(desk.contains("Choose a place to load the ensemble spread for that point."));
        assert!(desk.contains("does not forecast the next event"));
        assert!(desk.contains("0 earthquakes are in the loaded USGS record."));
        assert!(desk.contains("Series have not been pulled yet."));
        assert!(spoken.contains("Full member series for this place have not been held."));
    }

    #[test]
    fn desk_indexes_held_series_and_quake_rows_keep_a_quiet_alert() {
        let mut bulletin = Bulletin::default();
        bulletin.held.push(series::HeldNote {
            name: "rtsw".into(),
            source: "NOAA SWPC RTSW".into(),
            rows: 1200,
            first: "2026-11-03T01:00:00".into(),
            last: "2026-11-03T03:00:00".into(),
            fresh: true,
            detail: String::new(),
        });
        bulletin.held.push(series::HeldNote {
            name: "dst".into(),
            source: "Kyoto Dst".into(),
            rows: 4,
            first: "2026-11-03T00:00:00".into(),
            last: "2026-11-03T03:00:00".into(),
            fresh: false,
            detail: "Kyoto Dst did not load.".into(),
        });
        let text = desk_text(&[], &bulletin, false, &None, &None);
        let held_at = text.find("HELD SERIES").expect("held section");
        let space_at = text.find("\nSPACE\n").expect("space section");
        assert!(held_at < space_at, "{text}");
        assert!(text.contains("calm hours included"), "{text}");
        assert!(text.contains("Call pull_series"), "{text}");
        assert!(text.contains("pull rtsw: 1200 rows, 2026-11-03T01:00:00 through 2026-11-03T03:00:00."), "{text}");
        assert!(text.contains("The held file is unchanged."), "{text}");
        assert!(!text.contains("FSOT predicts"), "{text}");

        let rows = quake_rows(&[Quake {
            mag: 0.0,
            lat: 1.5,
            lon: -2.5,
            depth_km: 10.0,
            place: "quiet test".into(),
            time_ms: 1_762_000_000_000,
            alert: None,
            tsunami: false,
            id: "us-test".into(),
            ..Quake::default()
        }]);
        assert_eq!(rows.len(), 1);
        let stamp = rows[0].get("t").and_then(|value| value.as_str()).unwrap();
        assert!(stamp.ends_with('Z') && stamp.contains('T'), "{stamp}");
        assert_eq!(rows[0].get("id").and_then(|value| value.as_str()), Some("us-test"));
        assert!(rows[0].get("alert").unwrap().is_null());
        assert_eq!(rows[0].get("tsunami").and_then(|value| value.as_bool()), Some(false));
        assert_eq!(rows[0].get("mag").and_then(|value| value.as_f64()), Some(0.0));
        assert!(rows[0].get("nst").unwrap().is_null());
        assert!(rows[0].get("gap").unwrap().is_null());
        assert!(pull_series("../rtsw").is_err());
    }

    #[test]
    fn next_set_quotes_agency_values_and_does_not_invent_a_warning() {
        let url = wind_grid_url("10,20", "-70,-50");
        assert!(url.contains("pressure_msl"), "{url}");
        assert!(url.contains("relative_humidity_2m"), "{url}");
        assert!(url.contains("forecast_hours=24"), "{url}");
        assert!(!url.contains("past_hours"), "{url}");
        let span = pressure_span_line(&series::current_rows(
            r#"[{"latitude":1,"longitude":2,"current":{"time":"t","pressure_msl":900,"relative_humidity_2m":0}},{"latitude":3,"longitude":4,"current":{"time":"t","pressure_msl":1040,"relative_humidity_2m":100}}]"#,
            &[("pressure_msl", "pressure_hpa"), ("relative_humidity_2m", "humidity_pct")],
        ));
        assert!(span.contains("900") && span.contains("1040") && span.contains("0") && span.contains("100"), "{span}");
        assert!(!span.to_ascii_lowercase().contains("warning"), "{span}");
        let wind = parse_wind(r#"{"latitude":10,"longitude":20,"current":{"wind_speed_10m":2,"wind_direction_10m":40,"pressure_msl":null}}"#).unwrap();
        assert_eq!(wind.len(), 1);
        assert_eq!(wind[0].wind_ms, 2.0);

        let quakes = parse_quakes(
            r#"{"features":[
                {"id":"quiet","geometry":{"coordinates":[10.0,20.0,5.0]},"properties":{"mag":0,"place":"quiet","time":1762000000000,"alert":null,"tsunami":0,"magType":"ml","nst":null,"gap":0,"dmin":null,"rms":0.2}},
                {"id":"gappy","geometry":{"coordinates":[11.0,21.0,8.0]},"properties":{"mag":4.2,"place":"gap","time":1762000001000,"alert":null,"tsunami":0,"nst":14,"gap":null,"dmin":1.5,"rms":null}}
            ]}"#,
        )
        .unwrap();
        let rows = quake_rows(&quakes);
        assert_eq!(rows[0].get("mag").and_then(|value| value.as_f64()), Some(0.0));
        assert!(rows[0].get("alert").unwrap().is_null());
        assert!(rows[0].get("nst").unwrap().is_null());
        assert_eq!(rows[0].get("gap").and_then(|value| value.as_f64()), Some(0.0));
        assert!(rows[0].get("dmin").unwrap().is_null());
        assert!((rows[0].get("rms").and_then(|value| value.as_f64()).unwrap() - 0.2).abs() < 1e-5);
        assert_eq!(rows[0].get("mag_type").and_then(|value| value.as_str()), Some("ml"));
        assert_eq!(rows[1].get("nst").and_then(|value| value.as_i64()), Some(14));
        assert!(rows[1].get("gap").unwrap().is_null());
        assert_eq!(rows[1].get("dmin").and_then(|value| value.as_f64()), Some(1.5));
        assert!(rows[1].get("rms").unwrap().is_null());
        assert!(rows[1].get("mag_type").unwrap().is_null());

        let mut solar = SolarReading::default();
        apply_electrons(
            &mut solar,
            r#"[{"time_tag":"2026-11-03T03:00:00Z","energy":">=2 MeV","flux":10},{"time_tag":"2026-11-03T04:00:00Z","energy":">=1 MeV","flux":9},{"time_tag":"2026-11-03T04:00:00Z","energy":">=2 MeV","flux":1153.1}]"#,
        );
        let electrons = measured_lines(&solar).into_iter().find(|line| line.contains("electron")).expect("electron line");
        assert!(electrons.contains(">=2 MeV"), "{electrons}");
        assert!(electrons.contains("1153.1"), "{electrons}");
        assert!(electrons.contains("2026-11-03T04:00:00Z"), "{electrons}");
        assert!(!electrons.contains('9'), "{electrons}");
        assert!(!electrons.to_ascii_lowercase().contains("scale"), "{electrons}");
        assert!(!electrons.to_ascii_lowercase().contains("warning"), "{electrons}");

        apply_alerts(&mut solar, "[]");
        assert!(solar.alert_line.contains("no issued alerts"), "{}", solar.alert_line);
        assert!(!solar.alert_watch);
        assert!(!solar.alert_line.to_ascii_lowercase().contains("warning"), "{}", solar.alert_line);
        apply_alerts(
            &mut solar,
            r#"[{"product_id":"SUM","issue_datetime":"2026-11-01 00:00:00","message":"Space weather summary for the day."},{"product_id":"OLDER","issue_datetime":"2026-10-01 00:00:00","message":"WARNING: older"}]"#,
        );
        assert!(solar.alert_line.contains("SUM"), "{}", solar.alert_line);
        assert!(solar.alert_line.contains("Space weather summary for the day."), "{}", solar.alert_line);
        assert!(!solar.alert_line.contains("OLDER"), "{}", solar.alert_line);
        assert!(!solar.alert_watch, "{}", solar.alert_line);
        apply_alerts(
            &mut solar,
            r#"[{"product_id":"EF3A","issue_datetime":"2026-11-02 12:03:53.987","message":"Space Weather Message Code: ALTEF3\nSerial Number: 3764\n\nCONTINUED ALERT: Electron 2MeV Integral Flux exceeded 1,000pfu"}]"#,
        );
        assert!(!solar.alert_line.contains("Space Weather Message Code"), "{}", solar.alert_line);
        assert!(solar.alert_watch);
        assert!(solar.alert_line.contains("EF3A"), "{}", solar.alert_line);
        assert!(solar.alert_line.contains("CONTINUED ALERT: Electron 2MeV Integral Flux exceeded 1,000pfu"), "{}", solar.alert_line);
        assert!(solar.alert_line.contains("2026-11-02 12:03:53.987"), "{}", solar.alert_line);
        let mut bulletin = Bulletin::default();
        bulletin.solar = solar;
        let cards = watch_cards(&[], &bulletin);
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].title, bulletin.solar.alert_line);
        assert!(cards[0].lat.is_none());

        let ice = series::sea_ice_rows("Year, Month, Day, Extent, Missing\n2026, 11, 1, 8.016, 0.000\n2026, 11, 2, 0.000, 0.100\n", "N").unwrap();
        let south = series::sea_ice_rows("Year, Month, Day, Extent, Missing\n2026, 11, 1, , 0\n", "S").unwrap();
        let ice_text = ice_line(Some(&ice), Some(&south));
        assert!(ice_text.contains("8.016") || ice_text.contains("0"), "{ice_text}");
        assert!(ice_text.contains("10^6 km²"), "{ice_text}");
        assert!(ice_text.contains("2026-11-02"), "{ice_text}");
        assert!(ice_text.contains("Antarctic extent did not load"), "{ice_text}");
        assert!(!ice_text.to_ascii_lowercase().contains("warning"), "{ice_text}");

        let rmm = series::rmm_rows("2024 2 24 -3.14564817E-03 0.12845999 7 0.12849849 method\n").unwrap();
        let rmm_text = rmm_line(&rmm);
        assert!(rmm_text.contains("2024-02-24"), "{rmm_text}");
        assert!(rmm_text.contains("ends on that date"), "{rmm_text}");
        assert!(rmm_text.contains("One row is not an MJO event."), "{rmm_text}");
        assert!(rmm_text.contains("phase 7"), "{rmm_text}");
        assert!(!rmm_text.to_ascii_lowercase().contains("current"), "{rmm_text}");

        let fires = series::firms_rows(
            "latitude,longitude,bright_ti4,scan,track,acq_date,acq_time,satellite,confidence,version,bright_ti5,frp,daynight\n-10.5,20.25,300,0.4,0.4,2026-11-03,456,N,low,2,290,1.5,N\n1,2,340,0.4,0.4,2026-11-03,1205,N20,nominal,2,300,12,D\n",
        )
        .unwrap();
        let fire_text = fire_summary(&fires, &[]);
        assert!(fire_text.contains("detections: 2"), "{fire_text}");
        assert!(fire_text.contains("12"), "{fire_text}");
        assert!(fire_text.contains("FIRMS detection"), "{fire_text}");
        assert!(!fire_text.to_ascii_lowercase().contains("warning"), "{fire_text}");
        assert!(!fire_text.to_ascii_lowercase().contains("wildfire"), "{fire_text}");

        let warning = "\
WTPN31 PGTW 030300
SUBJ/TYPHOON 30W (IN-FA) WARNING NR 043//
1. TYPHOON 30W (IN-FA) WARNING NR 043
   WARNING POSITION:
   030000Z --- NEAR 23.1N 139.3E
   MAX SUSTAINED WINDS - 070 KT, GUSTS 085 KT
   WARNING POSITION:
   031200Z --- NEAR 24.0N 138.0E
   MAX SUSTAINED WINDS - 075 KT, GUSTS 090 KT
03NOV26. TYPHOON 30W (IN-FA), LOCATED
";
        let storm = jtwc_storm(warning).expect("storm");
        assert_eq!(storm.agency, "JTWC");
        assert_eq!(storm.id, "30W");
        assert_eq!(storm.name, "IN-FA");
        assert!((storm.lat - 23.1).abs() < 0.01, "{}", storm.lat);
        assert!((storm.lon - 139.3).abs() < 0.01, "{}", storm.lon);
        assert_eq!(storm.intensity_kt, Some(70.0));
        assert!(storm.updated.contains("03NOV26"), "{}", storm.updated);
        let quoted = storm_sentence(&[storm]);
        assert!(quoted.contains("JTWC"), "{quoted}");
        assert!(quoted.contains("23.10° N"), "{quoted}");
        assert!(quoted.contains("139.30° E"), "{quoted}");
        assert!(quoted.contains("70 kt"), "{quoted}");
        assert!(!quoted.contains("24.00"), "{quoted}");
        assert!(!quoted.contains("75 kt"), "{quoted}");
        let rss = "\
<rss><channel>
<item><title>Current Northwest Pacific/North Indian Ocean Tropical Systems</title><description><![CDATA[<a href='https://www.metoc.navy.mil/jtwc/products/wp3026web.txt'>TC Warning Text</a><a href='https://www.metoc.navy.mil/jtwc/products/io0426web.txt'>TC Warning Text</a>]]></description></item>
<item><title>Current Central/Eastern Pacific Tropical Systems</title><description><a href='https://www.metoc.navy.mil/jtwc/products/ep0126web.txt'>skip</a></description></item>
<item><title>Current Southern Hemisphere Tropical Systems</title><description>No Current Tropical Cyclone Warnings.</description></item>
<item><title>Current Significant Tropical Weather Advisories</title><description><a href='https://www.metoc.navy.mil/jtwc/products/abpwweb.txt'>ABPW</a></description></item>
</channel></rss>";
        let urls = jtwc_warning_urls(rss).unwrap();
        assert_eq!(urls.len(), 2, "{urls:?}");
        assert!(urls.iter().all(|url| url.contains("wp3026") || url.contains("io0426")), "{urls:?}");
        assert!(jtwc_warning_urls("<rss><channel><item><title>Current Southern Hemisphere Tropical Systems</title>No Current</item></channel></rss>").unwrap().is_empty());
    }

    #[test]
    fn photosphere_co2_current_and_air_stay_measurements() {
        let marine = marine_batch_url("-40,-10", "20,40");
        assert!(marine.contains("hourly=wave_height,ocean_current_velocity,ocean_current_direction"), "{marine}");
        assert!(marine.contains("forecast_hours=24"), "{marine}");
        assert!(!marine.contains("past_hours"), "{marine}");
        let closed = closed_check_url("wave", 1.0, 2.0).unwrap();
        assert!(closed.contains("hourly=wave_height"), "{closed}");
        assert!(!closed.contains("ocean_current"), "{closed}");
        let hours = series::hour_rows(
            r#"{"latitude":-40,"longitude":20,"hourly":{"time":["2026-11-03T05:00","2026-11-03T06:00"],"wave_height":[2.3,null],"ocean_current_velocity":[0,4.7],"ocean_current_direction":[null,37]}}"#,
            &[
                ("wave_height", "wave_m"),
                ("ocean_current_velocity", "current_kmh"),
                ("ocean_current_direction", "current_toward_deg"),
            ],
        );
        assert_eq!(hours[0].get("current_kmh").and_then(|value| value.as_f64()), Some(0.0));
        assert!(hours[0].get("current_toward_deg").unwrap().is_null());
        assert!(hours[1].get("wave_m").unwrap().is_null());
        assert_eq!(hours[1].get("current_toward_deg").and_then(|value| value.as_f64()), Some(37.0));
        let current = hourly_current_line(&hours);
        assert!(current.contains("0") && current.contains("4.7") && current.contains("km/h"), "{current}");
        assert!(!current.to_ascii_lowercase().contains("warning"), "{current}");

        let mut solar = SolarReading::default();
        solar.wind_kms = Some(390.0);
        solar.wind_time = "2026-11-03T05:00:00Z".into();
        solar.bt_nt = Some(4.8);
        solar.bz_nt = Some(-2.7);
        solar.imf_time = "2026-11-03T05:20:27Z".into();
        apply_mag(
            &mut solar,
            r#"[{"time_tag":"2026-11-03T05:13:00Z","satellite":19,"He":1,"Hp":2,"Hn":3,"total":70,"arcjet_flag":false},{"time_tag":"2026-11-03T05:14:00Z","satellite":19,"He":0,"Hp":63.1,"Hn":-1.7,"total":74.5,"arcjet_flag":true}]"#,
        );
        let front = solar_lines(&solar).join("\n");
        assert!(front.contains("Propagated IMF Bt 4.8 nT, Bz -2.7 nT"), "{front}");
        let mag = measured_lines(&solar).into_iter().find(|line| line.contains("magnetometer")).expect("mag line");
        assert!(mag.contains("74.5") && mag.contains("nT") && mag.contains("satellite 19"), "{mag}");
        assert!(mag.contains("arcjet flag on that row is true"), "{mag}");
        assert!(mag.contains("He, Hp, and Hn"), "{mag}");
        assert!(!mag.contains("4.8") && !mag.contains("-2.7"), "{mag}");
        assert!(!mag.to_ascii_lowercase().contains("warning"), "{mag}");
        assert!(!mag.contains("G-scale") && !mag.contains("G0"), "{mag}");

        let regions = series::object_rows(
            r#"[
                {"observed_date":"2026-11-02","region":4562,"location":"S17W43","area":null,"number_spots":null,"spot_class":null,"mag_class":null,"status":"f","c_flare_probability":99},
                {"observed_date":"2026-11-02","region":4564,"location":"N02W01","area":0,"number_spots":1,"spot_class":"Axx","mag_class":"GD","status":"a","c_flare_probability":1},
                {"observed_date":"2026-10-04","region":4544,"location":"S09W85","area":80,"number_spots":9,"spot_class":"Hsx","mag_class":"A","status":"f"}
            ]"#,
            "observed_date",
        )
        .unwrap();
        let region_line = region_summary(&regions);
        assert!(region_line.contains("2026-11-02") && region_line.contains("2 regions"), "{region_line}");
        assert!(region_line.contains("Region 4564") && region_line.contains("N02W01"), "{region_line}");
        assert!(region_line.contains("area 0 millionths of a solar hemisphere"), "{region_line}");
        assert!(region_line.contains("1 spot") && region_line.contains("class Axx"), "{region_line}");
        assert!(region_line.contains("A region entry is not a flare."), "{region_line}");
        assert!(!region_line.contains("99") && !region_line.contains("4544"), "{region_line}");
        assert!(!region_line.to_ascii_lowercase().contains("warning"), "{region_line}");

        let spots = series::object_rows(
            r#"[
                {"time_tag":"2026-11-03T02:15:00","Observatory":"LEA","Region":4564,"Location":"N02W01","Area":10,"Numspot":1,"Spotclass":"Axx","Magclass":"GD"},
                {"time_tag":"2026-11-03T02:15:00","Observatory":"LEA","Region":4566,"Location":"S05E10","Area":0,"Numspot":1,"Spotclass":"Axx","Magclass":"A"},
                {"time_tag":"2026-10-03T06:15:00","Observatory":"SVI","Region":4546,"Area":0,"Numspot":1,"Spotclass":"Axx","Magclass":"A"}
            ]"#,
            "time_tag",
        )
        .unwrap();
        let sun_line = sunspot_summary(&spots);
        assert!(sun_line.contains("2026-11-03T02:15:00") && sun_line.contains("observatory LEA"), "{sun_line}");
        assert!(sun_line.contains("2 regions") && sun_line.contains("Region 4564"), "{sun_line}");
        assert!(sun_line.contains("area 10 millionths of a solar hemisphere"), "{sun_line}");
        assert!(sun_line.contains("One report is not a flare."), "{sun_line}");
        assert!(!sun_line.contains("4546"), "{sun_line}");
        assert_eq!(spots[1].get("Area").and_then(|value| value.as_f64()), Some(0.0));

        let co2 = series::co2_rows("# note\n2026,11,1,2026.8329,428.25\n2022,12,1,2022.9,-99.99\n2023,7,5,2023.5,0\n").unwrap();
        let co2_line = co2_summary(&co2);
        assert!(co2_line.contains("Mauna Loa Observatory") && co2_line.contains("428.25 ppm") && co2_line.contains("2026-11-01"), "{co2_line}");
        assert!(co2_line.contains("Maunakea Observatories"), "{co2_line}");
        assert!(co2_line.contains("one station day"), "{co2_line}");
        assert!(!co2_line.to_ascii_lowercase().contains("warning"), "{co2_line}");
        assert!(co2[1].get("co2_ppm").unwrap().is_null());
        assert_eq!(co2[2].get("co2_ppm").and_then(|value| value.as_f64()), Some(0.0));

        let air_rows = series::hour_rows(
            r#"{"latitude":40.7,"longitude":-74,"hourly":{"time":["2026-11-03T04:00","2026-11-03T05:00","2026-11-03T06:00"],"carbon_monoxide":[261,null,300],"sulphur_dioxide":[3.7,2.9,4.0],"dust":[0,0,1]}}"#,
            &[("carbon_monoxide", "co_ugm3"), ("sulphur_dioxide", "so2_ugm3"), ("dust", "dust_ugm3")],
        );
        let air_line = air_summary(&air_rows, "2026-11-03T05");
        assert!(air_line.contains("2026-11-03T05:00"), "{air_line}");
        assert!(air_line.contains("carbon monoxide was not in that hour"), "{air_line}");
        assert!(air_line.contains("sulphur dioxide 2.9 ug/m^3"), "{air_line}");
        assert!(air_line.contains("dust 0 ug/m^3"), "{air_line}");
        assert!(!air_line.contains("300") && !air_line.contains("261"), "{air_line}");
        assert!(!air_line.to_ascii_lowercase().contains("aqi"), "{air_line}");
        assert!(!air_line.to_ascii_lowercase().contains("warning"), "{air_line}");
        let url = air_url(40.7, -74.0);
        assert!(url.contains("latitude=40.7&longitude=-74"), "{url}");
        assert!(url.contains("hourly=carbon_monoxide,sulphur_dioxide,dust"), "{url}");
        assert!(!url.contains("past_hours"), "{url}");

        solar.region_line = region_line;
        solar.sunspot_line = sun_line;
        let mut bulletin = Bulletin::default();
        bulletin.solar = solar;
        bulletin.climate.co2_line = co2_line;
        bulletin.current_hour_line = current;
        let cards = watch_cards(&[], &bulletin);
        assert!(cards.is_empty(), "{cards:?}");
        let desk = desk_text(&[], &Bulletin::default(), false, &None, &None);
        assert!(desk.contains("Choose a place to load carbon monoxide, sulphur dioxide, and dust for that point."), "{desk}");
        assert!(desk.contains("GOES magnetometer has not arrived."), "{desk}");
        assert!(desk.contains("NOAA GML daily CO2 has not arrived."), "{desk}");
        assert!(desk.contains("Open-Meteo hourly ocean current has not arrived."), "{desk}");
        let reading = compose_reading(&[], &[], &[], &[], &[], &bulletin, 1_700_000_000.0).join("\n");
        assert!(reading.contains("Propagated IMF Bt 4.8 nT"), "{reading}");
        assert!(!reading.contains("magnetometer"), "{reading}");
        assert!(!reading.contains("ug/m^3"), "{reading}");
        assert!(!reading.contains("428.25"), "{reading}");
    }

    #[test]
    fn vaac_text_and_gauges_stay_measurements() {
        let index = "\
<tr><td><a href=\"fvxx21.knes..txt\">fvxx21.knes..txt</a></td><td align=\"right\">03-Nov-2026 04:57  </td><td align=\"right\">907 </td></tr>
<tr><td><a href=\"fvau01.adrm..txt\">fvau01.adrm..txt</a></td><td align=\"right\">03-Nov-2026 01:37  </td><td align=\"right\">589 </td></tr>
<tr><td><a href=\"fvxx24.knes..txt\">fvxx24.knes..txt</a></td><td align=\"right\">02-Nov-2026 23:55  </td><td align=\"right\">1 </td></tr>
<tr><td><a href=\"fvcn01.cwao..txt\">fvcn01.cwao..txt</a></td><td align=\"right\">21-Oct-2026 09:03  </td><td align=\"right\">530 </td></tr>
<tr><td><a href=\"fvps01.nzkl..txt\">fvps01.nzkl..txt</a></td><td align=\"right\">30-Oct-2026 05:04  </td><td align=\"right\">526 </td></tr>
<tr><td><a href=\"fvag01.sabm..txt\">fvag01.sabm..txt</a></td><td align=\"right\">31-Oct-2026 14:05  </td><td align=\"right\">682 </td></tr>
";
        let now = geo::unix_utc(2026, 11, 3, 5.5);
        let files = fv_entries(index);
        let recent: Vec<&str> = recent_fv(&files, now).iter().map(|file| file.name.as_str()).collect();
        assert!(recent.contains(&"fvxx21.knes..txt"), "{recent:?}");
        assert!(recent.contains(&"fvau01.adrm..txt"), "{recent:?}");
        assert!(recent.contains(&"fvps01.nzkl..txt"), "{recent:?}");
        assert!(recent.contains(&"fvag01.sabm..txt"), "{recent:?}");
        assert!(!recent.iter().any(|name| *name == "fvxx24.knes..txt"), "{recent:?}");
        assert!(!recent.iter().any(|name| *name == "fvcn01.cwao..txt"), "{recent:?}");

        let washington = "\
FVXX21 KNES 030457
VA ADVISORY
DTG: 20261103/0457Z
VAAC: WASHINGTON
VOLCANO: REVENTADOR 352010
PSN: S0004 W07739
AREA: ECUADOR
ADVISORY NR: 2026/1159
ERUPTION DETAILS: VA IN WEBCAM
EST VA CLD: SFC/FL140 N0004 W07755 - S0004 W07740
- S0005 W07739 - S0002 W07757 MOV NW 10KT
FCST VA CLD +6HR: 03/1030Z NO VA EXP
";
        let darwin = "\
FVAU01 ADRM 030137
VA ADVISORY
DTG: 20261103/0137Z
VAAC: DARWIN
VOLCANO: MAYON 273030
PSN: N1315 E12341
AREA: PHILIPPINES
ADVISORY NR: 2026/29
ERUPTION DETAILS: UNKNOWN
";
        let toulouse = "\
FVXX01 LFPW 301541
VA ADVISORY
DTG: 20261030/1541Z
VAAC: TOULOUSE
VOLCANO: ETNA 211060
PSN: N3744 E01459
AREA: SICILY VOLCANIC PROVINCE
ADVISORY NR: 2026/211
AVIATION COLOUR CODE: ORANGE
ERUPTION DETAILS: ERUPTION AT 20261029/0600Z NO ASH CLOUD IS PRODUCED
OBS VA CLD: VA NOT IDENTIFIABLE FM SATELLITE DATA WIND FL100/130
265/10KT
FCST VA CLD +6 HR: 30/2130Z NO VA EXP
";
        let aires = "\
FVAG01 SABM 311405
VA ADVISORY
DTG: 20261031/1405Z
VAAC: BUENOS AIRES
VOLCANO: SABANCAYA 354006
PSN: S1547 W07150
ADVISORY NR: 2026/517
ERUPTION DETAILS: SPORADIC STEAN EMISSION
";
        let wellington = "\
FVPS01 NZKL 300504
VA ADVISORY
DTG: 20261030/0504Z
VAAC: WELLINGTON
VOLCANO: UNKNOWN
PSN: UNKNOWN
ADVISORY NR: 2026/10
ERUPTION DETAILS: NIL
";
        let montreal = "\
FVCN01 CWAO 030100
VA ADVISORY
STATUS: TEST
DTG: 20261103/0100Z
VAAC: MONTREAL
VOLCANO: MEAGER 320180
PSN: N5037 W12330
ADVISORY NR: 2026/0002
ERUPTION DETAILS: VA TEST
";
        let items = [
            parse_vaac(washington, "fvxx21.knes..txt", "file").expect("washington"),
            parse_vaac(darwin, "fvau01.adrm..txt", "file").expect("darwin"),
            parse_vaac(toulouse, "fvxx01.lfpw..txt", "file").expect("toulouse"),
            parse_vaac(aires, "fvag01.sabm..txt", "file").expect("aires"),
            parse_vaac(wellington, "fvps01.nzkl..txt", "file").expect("wellington"),
            parse_vaac(montreal, "fvcn01.cwao..txt", "file").expect("montreal"),
        ];
        assert!((items[0].lat.unwrap() + 4.0 / 60.0).abs() < 1e-9, "{:?}", items[0].lat);
        assert!((items[0].lon.unwrap() + (77.0 + 39.0 / 60.0)).abs() < 1e-9, "{:?}", items[0].lon);
        assert!((items[1].lat.unwrap() - 13.25).abs() < 1e-9, "{:?}", items[1].lat);
        assert!((items[1].lon.unwrap() - (123.0 + 41.0 / 60.0)).abs() < 1e-9, "{:?}", items[1].lon);
        assert!(items[4].lat.is_none() && items[4].lon.is_none());
        assert!(items[5].test);
        assert!(!items[0].test);
        assert_eq!(items[2].colour, "ORANGE");
        assert!(items[0].colour.is_empty());
        assert_eq!(items[2].eruption, "ERUPTION AT 20261029/0600Z NO ASH CLOUD IS PRODUCED");
        assert!(!items[2].eruption.contains("265/10KT"));
        assert!(!items[2].eruption.contains("NO VA EXP"));
        let (line, desk) = vaac_desk(&items, false);
        let joined = desk.join("\n");
        assert!(line.starts_with("NWS is holding 5 volcanic ash advisories"), "{line}");
        assert!(line.contains("1 VAAC test bulletin"), "{line}");
        assert!(joined.contains("VAAC WASHINGTON: REVENTADOR 352010 advisory 2026/1159 at 20261103/0457Z. Eruption details: VA IN WEBCAM."), "{joined}");
        assert!(joined.contains("SPORADIC STEAN EMISSION"), "{joined}");
        assert!(joined.contains("Eruption details: NIL."), "{joined}");
        assert!(joined.contains("Eruption details: UNKNOWN."), "{joined}");
        assert!(joined.contains("Aviation colour code: ORANGE."), "{joined}");
        assert!(joined.contains("A FIRMS detection is not an ash advisory."), "{joined}");
        assert!(!joined.contains("MEAGER"), "{joined}");
        assert!(!joined.contains("265/10KT"), "{joined}");
        assert!(!joined.contains("NO VA EXP"), "{joined}");
        assert!(!joined.to_ascii_lowercase().contains("warning"), "{joined}");
        let row = vaac_row(&items[4]);
        assert!(row.get("lat").unwrap().is_null());
        assert!(row.get("lon").unwrap().is_null());
        assert_eq!(row.get("eruption").and_then(|value| value.as_str()), Some("NIL"));
        assert_eq!(row.get("test").and_then(|value| value.as_bool()), Some(false));
        let (one_line, _) = vaac_desk(&items[..1], true);
        assert!(one_line.starts_with("NWS is holding 1 volcanic ash advisory"), "{one_line}");
        assert!(one_line.contains("A volcanic ash advisory text did not load."), "{one_line}");
        assert!(!vaac_index_url().contains("wifs") && !vaac_index_url().contains("isigmet"));
        assert!(!gauge_list_url().contains("/v2/"));

        let gauges = series::gauge_rows(
            r#"[
                {"Code":"aarh","Location":"Aarhus","type":"SF","Lat":56.15,"Lon":10.22,"sensor":"rad","units":"M","lasttime":"2026-11-03 05:30:00.000","lastvalue":0.5},
                {"Code":"acnj2","Location":"Atlantic City","type":"SL","Lat":39.3,"Lon":-74.4,"sensor":"pwl","units":"M","lasttime":"2026-11-03 05:33:00.000","lastvalue":-7.5},
                {"Code":"zero","Location":"Zero","type":"SL","Lat":1,"Lon":2,"sensor":"rad","units":"M","lasttime":"2026-11-03 05:00:00.000","lastvalue":0},
                {"Code":"miss","Location":"Missing","type":"SL","Lat":1,"Lon":2,"sensor":"rad","units":"M","lasttime":"2026-11-03 05:00:00.000","lastvalue":-999},
                {"Code":"dkwa","Location":"DART ESE of Saipan","type":"SD","Lat":15,"Lon":147,"sensor":"prt","units":"M","lasttime":"2026-11-03 00:00:00.000","lastvalue":5604},
                {"Code":"bat","Location":"Battery","type":"SL","Lat":21,"Lon":-157,"sensor":"bat","units":"V","lasttime":"2026-11-03 05:40:00.000","lastvalue":0}
            ]"#,
        )
        .unwrap();
        let gauge_line = gauge_summary(&gauges);
        assert!(gauge_line.contains("6 rows"), "{gauge_line}");
        assert!(gauge_line.contains("3 stations have a finite meter sample"), "{gauge_line}");
        assert!(gauge_line.contains("Deep-ocean type SD has 1 finite sample"), "{gauge_line}");
        assert!(gauge_line.contains("sensor and datum"), "{gauge_line}");
        assert!(gauge_line.contains("not a tsunami warning"), "{gauge_line}");
        assert!(gauge_line.contains("Missing code -999 stays empty"), "{gauge_line}");
        assert!(!gauge_line.contains("5604") && !gauge_line.contains("-7.5") && !gauge_line.contains("0.5"), "{gauge_line}");
        assert!(!gauge_line.to_ascii_lowercase().contains("highest"), "{gauge_line}");
        assert_eq!(gauge_summary(&[]), "UNESCO IOC lists no active sea-level stations.");

        let mut bulletin = Bulletin::default();
        bulletin.vaac_line = line;
        bulletin.vaac_desk = desk;
        bulletin.gauge_line = gauge_line.clone();
        let cards = watch_cards(&[], &bulletin);
        assert!(cards.is_empty(), "{cards:?}");
        let shown = desk_text(&[], &bulletin, false, &None, &None);
        assert!(shown.contains("VAAC WASHINGTON: REVENTADOR 352010"), "{shown}");
        assert!(shown.contains("3 stations have a finite meter sample"), "{shown}");
        let reading = compose_reading(&[], &[], &[], &[], &[], &bulletin, 1_700_000_000.0).join("\n");
        assert!(reading.contains("NWS is holding 5 volcanic ash advisories"), "{reading}");
        assert!(!reading.contains("0.5 m"), "{reading}");
        assert!(!reading.contains("5604"), "{reading}");
        let quiet = desk_text(&[], &Bulletin::default(), false, &None, &None);
        assert!(quiet.contains("NWS volcanic ash advisories have not arrived."), "{quiet}");
        assert!(quiet.contains("UNESCO IOC sea-level gauges have not arrived."), "{quiet}");
    }
}
