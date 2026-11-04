//! Daily append-only log written by the feed thread.
//!
//! The file name is the UTC date, `cache/record/YYYY-MM-DD.jsonl`.
//! Each line is one JSON object with `t` (UTC) and `k` (kind):
//! quake, volcano, storm, cme, tsunami, weekly, hour, result.
//! `state.json` remembers keys so a restart does not write the same event twice.
//! A line is added when something changes, once an hour, and once when a stored
//! forecast hour has closed.
//! A closed wind, rain, or wave hour uses an Open-Meteo value for that same hour.
//! The current grid sample is not that comparison.

use crate::feeds::{self, Bulletin, Quake, Volcano, WindSample};
use crate::geo;
use serde_json::{Map, Value};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

/// One Open-Meteo value for a stored forecast hour, queried at that hour's point.
#[derive(Clone, Debug)]
pub struct HourCheck {
    pub of: String,
    pub hour: String,
    pub value: f32,
}

pub fn write_kept_record(
    dir: &Path,
    label: &str,
    now_unix: f64,
    quakes: &[Quake],
    volcanoes: &[Volcano],
    wind: &[WindSample],
    faults: &[Vec<(f32, f32)>],
    bulletin: &Bulletin,
    checks: &[HourCheck],
) -> String {
    let day = utc_day(now_unix);
    let stamp = iso_utc(now_unix);
    let now_hour = hour_key(&stamp);
    if let Err(err) = fs::create_dir_all(dir) {
        return format!("Kept record did not write this pass ({err}).");
    }
    let path = dir.join(format!("{day}.jsonl"));
    let state_path = dir.join("state.json");
    let mut state = load_state(&state_path);
    let mut lines = Vec::new();

    close_forecasts(&mut state, &mut lines, &stamp, &now_hour, bulletin, checks);
    remember_forecasts(&mut state, &now_hour, bulletin);
    remember_events(&mut state, &mut lines, &stamp, quakes, volcanoes, bulletin);
    if state.get("hour").and_then(|v| v.as_str()) != Some(now_hour.as_str()) {
        lines.push(hour_line(&stamp, quakes, volcanoes, faults, wind, bulletin));
        state.insert("hour".into(), Value::String(now_hour.to_string()));
    }

    if let Err(err) = append_lines(&path, &lines) {
        return format!("Kept record did not write this pass ({err}).");
    }
    if let Err(err) = fs::write(&state_path, serde_json::to_string(&state).unwrap_or_else(|_| "{}".into())) {
        return format!("Kept record wrote the day file and did not update its index ({err}).");
    }
    let count = fs::read_to_string(&path).map(|text| text.lines().filter(|line| !line.is_empty()).count()).unwrap_or(0);
    format!("Kept record {label}/{day}.jsonl · {count} lines today.")
}

fn close_forecasts(
    state: &mut Map<String, Value>,
    lines: &mut Vec<Value>,
    stamp: &str,
    now_hour: &str,
    bulletin: &Bulletin,
    checks: &[HourCheck],
) {
    let pending = state.get("pending").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let mut keep = Vec::new();
    let now_index = hour_index(now_hour);
    for item in pending {
        let of = item.get("of").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let hour = item.get("hour").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let value = item.get("value").and_then(|v| v.as_f64()).unwrap_or(f64::NAN) as f32;
        if of.is_empty() || hour.is_empty() || !value.is_finite() {
            continue;
        }
        if hour.as_str() >= now_hour {
            keep.push(item);
            continue;
        }
        let key = format!("{of}|{hour}");
        if result_seen(state, &key) {
            continue;
        }
        let mut row = base(stamp, "result");
        row.insert("of".into(), Value::String(of.clone()));
        row.insert("hour".into(), Value::String(hour.clone()));
        put_f32(&mut row, "forecast", value);
        let wrote = match of.as_str() {
            "wind" | "wave" | "rain" => match matched_check(checks, &of, &hour) {
                Some(check) => {
                    let wrote = fill_sample(&mut row, Some((check.value, 0.0)));
                    if wrote {
                        row.insert("later_t".into(), Value::String(hour.clone()));
                        row.insert("src".into(), Value::String("open-meteo-hour".into()));
                    }
                    wrote
                }
                None => false,
            },
            "kp" => fill_kp(&mut row, hour.as_str(), bulletin),
            _ => false,
        };
        let aged = match (now_index, hour_index(&hour)) {
            (Some(now_h), Some(then)) => now_h.saturating_sub(then) >= 36,
            _ => false,
        };
        if wrote || aged {
            if !wrote {
                row.insert("gap".into(), Value::from(1));
            }
            remember_result(state, &key);
            lines.push(Value::Object(row));
        } else {
            keep.push(item);
        }
    }
    trim_array(state, "results", 400);
    state.insert("pending".into(), Value::Array(keep));
}

fn matched_check<'a>(checks: &'a [HourCheck], of: &str, hour: &str) -> Option<&'a HourCheck> {
    let hour = hour_key(hour);
    checks.iter().find(|check| check.of == of && hour_key(&check.hour) == hour && check.value.is_finite())
}

fn fill_sample(row: &mut Map<String, Value>, observation: Option<(f32, f32)>) -> bool {
    let Some((later, km)) = observation else { return false };
    put_f32(row, "later", later);
    put_f32(row, "km", km);
    true
}

fn fill_kp(row: &mut Map<String, Value>, hour: &str, bulletin: &Bulletin) -> bool {
    let Some(later) = bulletin.solar.kp_now else { return false };
    if hour_key(&bulletin.solar.kp_now_time).as_str() < hour {
        return false;
    }
    put_f32(row, "later", later);
    if !bulletin.solar.kp_now_time.is_empty() {
        row.insert("later_t".into(), Value::String(bulletin.solar.kp_now_time.clone()));
    }
    row.insert("src".into(), Value::String("noaa-kp".into()));
    true
}

fn remember_forecasts(state: &mut Map<String, Value>, now_hour: &str, bulletin: &Bulletin) {
    upsert_peak(state, "wind", now_hour, bulletin.wind_forecast.as_ref());
    upsert_peak(state, "wave", now_hour, bulletin.wave_forecast.as_ref());
    upsert_peak(state, "rain", now_hour, bulletin.rain_forecast.as_ref());
    if let Some(kp) = bulletin.solar.kp_forecast {
        let hour = hour_key(&bulletin.solar.kp_forecast_time);
        if hour.len() >= 13 && hour.as_str() >= now_hour {
            upsert(state, "kp", &hour, kp, 0.0, 0.0);
        }
    }
    trim_pending(state, 64);
}

fn upsert_peak(state: &mut Map<String, Value>, of: &str, now_hour: &str, peak: Option<&feeds::SamplePeak>) {
    let Some(peak) = peak else { return };
    let hour = hour_key(&peak.when);
    if hour.len() < 13 || hour.as_str() < now_hour {
        return;
    }
    upsert(state, of, &hour, peak.value, peak.lat, peak.lon);
}

fn upsert(state: &mut Map<String, Value>, of: &str, hour: &str, value: f32, lat: f32, lon: f32) {
    let pending = state.entry("pending").or_insert_with(|| Value::Array(Vec::new()));
    let Some(list) = pending.as_array_mut() else { return };
    if let Some(found) = list.iter_mut().find(|item| item.get("of").and_then(|v| v.as_str()) == Some(of) && item.get("hour").and_then(|v| v.as_str()) == Some(hour)) {
        if let Some(map) = found.as_object_mut() {
            put_f32(map, "value", value);
            put_f32(map, "lat", lat);
            put_f32(map, "lon", lon);
        }
        return;
    }
    list.push(serde_json::json!({
        "of": of,
        "hour": hour,
        "value": round3(value),
        "lat": round3(lat),
        "lon": round3(lon),
    }));
}

fn remember_events(
    state: &mut Map<String, Value>,
    lines: &mut Vec<Value>,
    stamp: &str,
    quakes: &[Quake],
    volcanoes: &[Volcano],
    bulletin: &Bulletin,
) {
    for quake in quakes {
        let id = quake_key(quake);
        if !seen_push(state, "quakes", &id, 2000) {
            continue;
        }
        let mut row = base(stamp, "quake");
        row.insert("id".into(), Value::String(id));
        put_f32(&mut row, "m", quake.mag);
        put_f32(&mut row, "d", quake.depth_km);
        put_f32(&mut row, "lat", quake.lat);
        put_f32(&mut row, "lon", quake.lon);
        let place = clip(&quake.place, 80);
        if !place.is_empty() {
            row.insert("place".into(), Value::String(place));
        }
        if quake.tsunami {
            row.insert("tsunami".into(), Value::from(1));
        }
        if let Some(alert) = &quake.alert {
            row.insert("alert".into(), Value::String(alert.clone()));
        }
        if let Some((name, km)) = nearest_holocene(quake.lat, quake.lon, volcanoes) {
            row.insert("near".into(), Value::String(name.to_string()));
            put_f32(&mut row, "near_km", km);
        }
        lines.push(Value::Object(row));
    }
    for alert in &bulletin.volcano_alerts {
        let key = format!("{}|{}|{}", alert.name, alert.alert_level, alert.color_code);
        if !seen_push(state, "volcano", &key, 400) {
            continue;
        }
        let mut row = base(stamp, "volcano");
        row.insert("name".into(), Value::String(alert.name.clone()));
        row.insert("level".into(), Value::String(alert.alert_level.clone()));
        row.insert("color".into(), Value::String(alert.color_code.clone()));
        put_f32(&mut row, "lat", alert.lat);
        put_f32(&mut row, "lon", alert.lon);
        lines.push(Value::Object(row));
    }
    for storm in &bulletin.storms {
        let kt = storm.intensity_kt.map(|v| format!("{v:.0}")).unwrap_or_default();
        let key = format!("{}|{}|{}|{kt}", storm.agency, storm.id, storm.class_code);
        if !seen_push(state, "storms", &key, 200) {
            continue;
        }
        let mut row = base(stamp, "storm");
        row.insert("id".into(), Value::String(storm.id.clone()));
        if !storm.agency.is_empty() {
            row.insert("agency".into(), Value::String(storm.agency.clone()));
        }
        if !storm.name.is_empty() {
            row.insert("name".into(), Value::String(storm.name.clone()));
        }
        if !storm.class_code.is_empty() {
            row.insert("class".into(), Value::String(storm.class_code.clone()));
        }
        put_f32(&mut row, "lat", storm.lat);
        put_f32(&mut row, "lon", storm.lon);
        if let Some(kt) = storm.intensity_kt {
            put_f32(&mut row, "kt", kt);
        }
        lines.push(Value::Object(row));
    }
    if bulletin.solar.cme_line.contains("km/s") && state.get("cme").and_then(|v| v.as_str()) != Some(bulletin.solar.cme_line.as_str()) {
        let mut row = base(stamp, "cme");
        row.insert("line".into(), Value::String(clip(&bulletin.solar.cme_line, 180)));
        lines.push(Value::Object(row));
        state.insert("cme".into(), Value::String(bulletin.solar.cme_line.clone()));
    }
    for id in &bulletin.tsunami_ids {
        if !seen_push(state, "tsunami", id, 100) {
            continue;
        }
        let mut row = base(stamp, "tsunami");
        row.insert("id".into(), Value::String(id.clone()));
        row.insert("line".into(), Value::String(clip(&bulletin.tsunami_line, 180)));
        lines.push(Value::Object(row));
    }
    for report in &bulletin.weekly {
        let key = if report.guid.is_empty() {
            format!("{}|{}|{}", report.week, report.name, report.kind)
        } else {
            report.guid.clone()
        };
        if !seen_push(state, "weekly", &key, 400) {
            continue;
        }
        let mut row = base(stamp, "weekly");
        row.insert("name".into(), Value::String(report.name.clone()));
        row.insert("country".into(), Value::String(report.country.clone()));
        row.insert("week".into(), Value::String(report.week.clone()));
        row.insert("kind".into(), Value::String(report.kind.clone()));
        put_f32(&mut row, "lat", report.lat);
        put_f32(&mut row, "lon", report.lon);
        lines.push(Value::Object(row));
    }
}

fn hour_line(
    stamp: &str,
    quakes: &[Quake],
    volcanoes: &[Volcano],
    faults: &[Vec<(f32, f32)>],
    wind: &[WindSample],
    bulletin: &Bulletin,
) -> Value {
    let mut row = base(stamp, "hour");
    if let Some(speed) = bulletin.solar.wind_kms {
        put_f32(&mut row, "sw", speed);
    }
    if let Some(kp) = bulletin.solar.kp_now {
        put_f32(&mut row, "kp", kp);
    }
    if !bulletin.solar.kp_now_kind.is_empty() {
        row.insert("kp_kind".into(), Value::String(bulletin.solar.kp_now_kind.clone()));
    }
    if let Some(kp) = bulletin.solar.kp_forecast {
        put_f32(&mut row, "kp_f", kp);
        if !bulletin.solar.kp_forecast_time.is_empty() {
            row.insert("kp_ft".into(), Value::String(hour_key(&bulletin.solar.kp_forecast_time)));
        }
    }
    if !bulletin.solar.xray_class.is_empty() {
        row.insert("xray".into(), Value::String(bulletin.solar.xray_class.clone()));
    }
    if let Some(peak) = &bulletin.wind_forecast {
        put_f32(&mut row, "wind", peak.value);
        row.insert("wind_h".into(), Value::String(hour_key(&peak.when)));
    }
    if let Some(peak) = &bulletin.wave_forecast {
        put_f32(&mut row, "wave", peak.value);
    }
    if let Some(peak) = &bulletin.rain_forecast {
        put_f32(&mut row, "rain", peak.value);
    }
    if bulletin.solar.cme_line.contains("km/s") {
        row.insert("cme".into(), Value::from(1));
    }
    if let Some(quake) = quakes.iter().max_by(|a, b| a.mag.total_cmp(&b.mag)) {
        put_f32(&mut row, "qm", quake.mag);
        if let Some((name, km)) = nearest_holocene(quake.lat, quake.lon, volcanoes) {
            row.insert("near".into(), Value::String(name.to_string()));
            put_f32(&mut row, "near_km", km);
        }
        if let Some(km) = nearest_fault_km(quake.lat, quake.lon, faults) {
            put_f32(&mut row, "fault_km", km);
        }
    }
    if let Some(sample) = wind.iter().filter_map(|sample| sample.precip_mm.map(|mm| (sample, mm))).max_by(|a, b| a.1.total_cmp(&b.1)) {
        put_f32(&mut row, "rain_now", sample.1);
    }
    put_measured(&mut row, bulletin);
    Value::Object(row)
}

fn put_measured(row: &mut Map<String, Value>, bulletin: &Bulletin) {
    let solar = &bulletin.solar;
    if let Some(value) = solar.density_cm3 {
        put_f32(row, "np", value);
    }
    if let Some(value) = solar.temperature_k {
        put_f32(row, "tp", value);
    }
    if !solar.plasma_time.is_empty() {
        row.insert("np_t".into(), Value::String(solar.plasma_time.clone()));
    }
    if let Some(value) = solar.proton_10mev {
        put_f32(row, "p10", value);
    }
    if !solar.proton_10mev_time.is_empty() {
        row.insert("p10_t".into(), Value::String(solar.proton_10mev_time.clone()));
    }
    if let Some(value) = solar.f107_sfu {
        put_f32(row, "f107", value);
    }
    if let Some(value) = solar.f107_mean_sfu {
        put_f32(row, "f107m", value);
    }
    if let Some(value) = solar.dst_nt {
        put_f32(row, "dst", value);
    }
    if !solar.scale_r.is_empty() {
        row.insert("noaa_r".into(), Value::String(solar.scale_r.clone()));
    }
    if !solar.scale_s.is_empty() {
        row.insert("noaa_s".into(), Value::String(solar.scale_s.clone()));
    }
    if !solar.scale_g.is_empty() {
        row.insert("noaa_g".into(), Value::String(solar.scale_g.clone()));
    }
    let climate = &bulletin.climate;
    if let Some(value) = climate.oni_anom {
        put_f32(row, "oni", value);
    }
    if let Some(year) = climate.oni_year {
        if !climate.oni_season.is_empty() {
            row.insert("oni_t".into(), Value::String(format!("{} {year}", climate.oni_season)));
        }
    }
    if let Some(value) = climate.ao {
        put_f32(row, "ao", value);
    }
    if let Some(value) = climate.nao {
        put_f32(row, "nao", value);
    }
}

fn nearest_holocene<'a>(lat: f32, lon: f32, volcanoes: &'a [Volcano]) -> Option<(&'a str, f32)> {
    volcanoes
        .iter()
        .filter(|volcano| volcano.last_year.unwrap_or(i32::MIN) >= 2000)
        .map(|volcano| (volcano.name.as_str(), feeds::km_between(lat, lon, volcano.lat, volcano.lon)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

fn nearest_fault_km(lat: f32, lon: f32, faults: &[Vec<(f32, f32)>]) -> Option<f32> {
    let mut best = None;
    for chain in faults {
        for (flat, flon) in chain {
            let km = feeds::km_between(lat, lon, *flat, *flon);
            best = Some(best.map(|current: f32| current.min(km)).unwrap_or(km));
        }
    }
    best
}

fn quake_key(quake: &Quake) -> String {
    if !quake.id.is_empty() {
        return quake.id.clone();
    }
    format!("{:.1}|{}|{:.2}|{:.2}", quake.mag, quake.time_ms, quake.lat, quake.lon)
}

fn seen_push(state: &mut Map<String, Value>, field: &str, key: &str, cap: usize) -> bool {
    let list = state.entry(field).or_insert_with(|| Value::Array(Vec::new()));
    let Some(list) = list.as_array_mut() else { return false };
    if list.iter().any(|item| item.as_str() == Some(key)) {
        return false;
    }
    list.push(Value::String(key.to_string()));
    if list.len() > cap {
        let drop_n = list.len() - cap;
        list.drain(0..drop_n);
    }
    true
}

fn result_seen(state: &Map<String, Value>, key: &str) -> bool {
    state.get("results").and_then(|v| v.as_array()).is_some_and(|list| list.iter().any(|item| item.as_str() == Some(key)))
}

fn remember_result(state: &mut Map<String, Value>, key: &str) {
    let list = state.entry("results").or_insert_with(|| Value::Array(Vec::new()));
    if let Some(list) = list.as_array_mut() {
        list.push(Value::String(key.to_string()));
    }
}

fn trim_array(state: &mut Map<String, Value>, field: &str, cap: usize) {
    let Some(list) = state.get_mut(field).and_then(|v| v.as_array_mut()) else { return };
    if list.len() > cap {
        let drop_n = list.len() - cap;
        list.drain(0..drop_n);
    }
}

fn trim_pending(state: &mut Map<String, Value>, cap: usize) {
    trim_array(state, "pending", cap);
}

fn base(stamp: &str, kind: &str) -> Map<String, Value> {
    let mut row = Map::new();
    row.insert("t".into(), Value::String(stamp.to_string()));
    row.insert("k".into(), Value::String(kind.to_string()));
    row
}

fn put_f32(row: &mut Map<String, Value>, key: &str, value: f32) {
    if let Some(number) = serde_json::Number::from_f64(round3(value)) {
        row.insert(key.into(), Value::Number(number));
    }
}

fn round3(value: f32) -> f64 {
    (f64::from(value) * 1000.0).round() / 1000.0
}

fn load_state(path: &Path) -> Map<String, Value> {
    let Ok(text) = fs::read_to_string(path) else {
        return Map::new();
    };
    serde_json::from_str::<Value>(&text).ok().and_then(|value| value.as_object().cloned()).unwrap_or_default()
}

fn append_lines(path: &Path, lines: &[Value]) -> Result<(), String> {
    if lines.is_empty() {
        if !path.exists() {
            OpenOptions::new().create(true).append(true).open(path).map_err(|err| err.to_string())?;
        }
        return Ok(());
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path).map_err(|err| err.to_string())?;
    for line in lines {
        let mut text = serde_json::to_string(line).map_err(|err| err.to_string())?;
        text.push('\n');
        file.write_all(text.as_bytes()).map_err(|err| err.to_string())?;
    }
    Ok(())
}

fn clip(text: &str, n: usize) -> String {
    text.chars().take(n).collect()
}

fn utc_day(unix: f64) -> String {
    let days = (unix.floor() as i64).div_euclid(86400);
    let (year, month, day) = geo::civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

fn iso_utc(unix: f64) -> String {
    let secs = unix.floor() as i64;
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (year, month, day) = geo::civil_from_days(days);
    let hour = rem / 3600;
    let minute = (rem % 3600) / 60;
    let second = rem % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn hour_key(text: &str) -> String {
    text.chars().take(13).collect()
}

pub fn look_lines(dir: &Path, now_unix: f64) -> Vec<String> {
    let now_hour = hour_key(&iso_utc(now_unix));
    let mut results = Vec::new();
    for day in [utc_day(now_unix - 86400.0), utc_day(now_unix)] {
        let Ok(text) = fs::read_to_string(dir.join(format!("{day}.jsonl"))) else { continue };
        for line in text.lines() {
            let Ok(value) = serde_json::from_str::<Value>(line) else { continue };
            if value.get("k").and_then(|v| v.as_str()) != Some("result") {
                continue;
            }
            if let Some(sentence) = result_sentence(&value) {
                results.push(sentence);
            }
        }
    }
    if results.len() > 8 {
        let drop_n = results.len() - 8;
        results.drain(0..drop_n);
    }
    let mut ahead = Vec::new();
    let mut waiting = Vec::new();
    if let Ok(text) = fs::read_to_string(dir.join("state.json")) {
        if let Ok(state) = serde_json::from_str::<Value>(&text) {
            if let Some(pending) = state.get("pending").and_then(|v| v.as_array()) {
                for item in pending {
                    let Some(phrase) = pending_phrase(item) else { continue };
                    let hour = item.get("hour").and_then(|v| v.as_str()).unwrap_or("");
                    if hour_key(hour).as_str() < now_hour.as_str() {
                        waiting.push(phrase);
                    } else {
                        ahead.push(phrase);
                    }
                }
            }
        }
    }
    let mut lines = Vec::new();
    lines.extend(results);
    if !ahead.is_empty() {
        lines.push(format!("Still ahead: {}.", ahead.join("; ")));
    }
    if !waiting.is_empty() {
        lines.push(format!("Closed hour waiting for the agency value: {}.", waiting.join("; ")));
    }
    if lines.is_empty() {
        vec!["Forecast check: no stored forecast hour has a later agency value yet.".into()]
    } else {
        let mut out = vec!["Forecast check:".into()];
        out.extend(lines);
        out
    }
}

fn pending_phrase(item: &Value) -> Option<String> {
    let of = item.get("of")?.as_str()?;
    let hour = item.get("hour")?.as_str()?;
    let value = item.get("value")?.as_f64()?;
    Some(format!("{} {} hour {hour} at {}", agency_name(of), of_label(of), measure(of, value)))
}

fn result_sentence(value: &Value) -> Option<String> {
    let of = value.get("of")?.as_str()?;
    let hour = value.get("hour")?.as_str()?;
    let forecast = value.get("forecast")?.as_f64()?;
    let agency = agency_name(of);
    let label = of_label(of);
    let gap = value.get("gap").and_then(|v| v.as_i64()) == Some(1);
    let later = value.get("later").and_then(|v| v.as_f64());
    if later.is_none() && gap {
        return Some(format!(
            "{agency} {label} hour {hour} is behind the kept record. The stored forecast was {}. No later value for that hour was loaded.",
            measure(of, forecast)
        ));
    }
    let later = later?;
    let mut sentence = format!(
        "{agency} later value for {label} hour {hour} is {}. The stored forecast was {}.",
        measure(of, later),
        measure(of, forecast)
    );
    if let Some(when) = value.get("later_t").and_then(|v| v.as_str()).filter(|text| !text.is_empty()) {
        sentence.push_str(&format!(" Later time {when}."));
    }
    sentence.push_str(" That later number is the agency's value for the hour.");
    Some(sentence)
}

fn agency_name(of: &str) -> &'static str {
    match of {
        "kp" => "NOAA SWPC",
        _ => "Open-Meteo",
    }
}

fn of_label(of: &str) -> &str {
    match of {
        "kp" => "Kp",
        other => other,
    }
}

fn measure(of: &str, value: f64) -> String {
    let number = format_num(value);
    match of {
        "wind" => format!("{number} m/s"),
        "wave" => format!("{number} m"),
        "rain" => format!("{number} mm"),
        "kp" => format!("Kp {number}"),
        _ => number,
    }
}

fn format_num(value: f64) -> String {
    let rounded = (value * 1000.0).round() / 1000.0;
    if (rounded - rounded.round()).abs() < 1e-9 {
        format!("{}", rounded.round() as i64)
    } else {
        format!("{rounded:.3}").trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

fn hour_index(stamp: &str) -> Option<i64> {
    let stamp = hour_key(stamp);
    if stamp.len() < 13 {
        return None;
    }
    let year: i32 = stamp[0..4].parse().ok()?;
    let month: u32 = stamp[5..7].parse().ok()?;
    let day: u32 = stamp[8..10].parse().ok()?;
    let hour: i64 = stamp[11..13].parse().ok()?;
    Some(geo::days_from_civil(year, month, day) * 24 + hour)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feeds::{SamplePeak, SolarReading, VolcanoAlert};

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("fsot-record-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn bulletin() -> Bulletin {
        let mut bulletin = Bulletin {
            wind_forecast: Some(SamplePeak { lat: 10.0, lon: 10.0, value: 12.0, when: "2026-11-03T06:00".into() }),
            wave_forecast: Some(SamplePeak { lat: 10.0, lon: 12.0, value: 4.0, when: "2026-11-03T08:00".into() }),
            rain_forecast: Some(SamplePeak { lat: 10.0, lon: 10.0, value: 3.0, when: "2026-11-03T07:00".into() }),
            ..Bulletin::default()
        };
        bulletin.solar = SolarReading {
            kp_now: Some(2.0),
            kp_now_kind: "estimated".into(),
            kp_now_time: "2023-11-14T22:00:00".into(),
            kp_forecast: Some(3.0),
            kp_forecast_time: "2026-11-03T00:00:00".into(),
            cme_line: "SIDC CACTus LASCO CME 0012 onset 2026/11/02 00:36, median speed 151 km/s, width 8°.".into(),
            density_cm3: Some(1.78),
            temperature_k: Some(74806.0),
            proton_10mev: Some(0.224),
            f107_sfu: Some(98.0),
            dst_nt: Some(-8.0),
            scale_r: "0 none".into(),
            ..SolarReading::default()
        };
        bulletin.climate.oni_season = "JAS".into();
        bulletin.climate.oni_year = Some(2026);
        bulletin.climate.oni_anom = Some(2.16);
        bulletin.climate.ao = Some(1.1556);
        bulletin.climate.nao = Some(0.5334);
        bulletin
    }

    #[test]
    fn record_keeps_one_line_per_event_and_scores_a_closed_hour_once() {
        let dir = scratch("dedup");
        let quakes = vec![Quake {
            id: "us-test".into(),
            mag: 5.1,
            lat: 0.0,
            lon: 0.0,
            depth_km: 10.0,
            place: "equator test".into(),
            time_ms: 1_700_000_000_000,
            ..Quake::default()
        }];
        let volcanoes = vec![Volcano { name: "Test".into(), lat: 0.0, lon: 0.2, elevation_m: 100, last_year: Some(2010) }];
        let alerts = vec![VolcanoAlert {
            name: "Kilauea".into(),
            lat: 19.4,
            lon: -155.3,
            alert_level: "ADVISORY".into(),
            color_code: "YELLOW".into(),
            synopsis: String::new(),
        }];
        let wind = vec![WindSample {
            lat: 10.0,
            lon: 10.0,
            temp_c: 20.0,
            wind_ms: 8.0,
            from_deg: 90.0,
            precip_mm: Some(1.0),
        }];
        let bulletin = bulletin();
        let mut bulletin = bulletin;
        bulletin.volcano_alerts = alerts;
        let first = write_kept_record(&dir, "record", 1_700_000_000.0, &quakes, &volcanoes, &wind, &[], &bulletin, &[]);
        assert!(first.contains("2023-11-14.jsonl"), "{first}");
        let again = write_kept_record(&dir, "record", 1_700_000_100.0, &quakes, &volcanoes, &wind, &[], &bulletin, &[]);
        assert!(again.contains("· 4 lines today.") || again.contains("lines today"), "{again}");
        let day = fs::read_to_string(dir.join("2023-11-14.jsonl")).unwrap();
        assert_eq!(day.matches("\"k\":\"quake\"").count(), 1, "{day}");
        assert_eq!(day.matches("\"k\":\"volcano\"").count(), 1, "{day}");
        assert_eq!(day.matches("\"k\":\"cme\"").count(), 1, "{day}");
        assert_eq!(day.matches("\"k\":\"hour\"").count(), 1, "{day}");
        assert!(day.contains("\"near\":\"Test\""), "{day}");
        assert!(day.contains("\"np\":1.78"), "{day}");
        assert!(day.contains("74806"), "{day}");
        assert!(day.contains("\"p10\":0.224"), "{day}");
        assert!(day.contains("\"f107\":98"), "{day}");
        assert!(day.contains("\"oni\":2.16"), "{day}");
        assert!(day.contains("\"oni_t\":\"JAS 2026\""), "{day}");
        assert!(day.contains("\"noaa_r\":\"0 none\""), "{day}");
        assert!(!day.contains("\"k\":\"result\""), "{day}");

        let mut later_bulletin = bulletin.clone();
        later_bulletin.solar.kp_now = Some(2.0);
        later_bulletin.solar.kp_now_time = "2026-11-03T09:00:00".into();
        let closed = geo::days_from_civil(2026, 11, 3) as f64 * 86400.0 + 9.0 * 3600.0;
        let checks = [
            HourCheck { of: "wind".into(), hour: "2026-11-03T06".into(), value: 4.25 },
            HourCheck { of: "rain".into(), hour: "2026-11-03T07".into(), value: 1.5 },
            HourCheck { of: "wave".into(), hour: "2026-11-03T08".into(), value: 3.5 },
        ];
        let scored = write_kept_record(&dir, "record", closed, &quakes, &volcanoes, &wind, &[], &later_bulletin, &checks);
        assert!(scored.contains("2026-11-03.jsonl"), "{scored}");
        let next = fs::read_to_string(dir.join("2026-11-03.jsonl")).unwrap();
        assert_eq!(next.matches("\"k\":\"quake\"").count(), 0, "{next}");
        assert_eq!(next.matches("\"of\":\"wind\"").count(), 1, "{next}");
        assert_eq!(next.matches("\"of\":\"rain\"").count(), 1, "{next}");
        assert_eq!(next.matches("\"of\":\"wave\"").count(), 1, "{next}");
        assert_eq!(next.matches("\"of\":\"kp\"").count(), 1, "{next}");
        assert!(next.contains("\"later\":4.25"), "{next}");
        assert!(next.contains("\"km\":0"), "{next}");
        assert!(next.contains("\"later_t\":\"2026-11-03T06\""), "{next}");
        assert!(next.contains("\"src\":\"open-meteo-hour\""), "{next}");
        assert!(!next.contains("\"later\":8"), "{next}");
        let spoken = look_lines(&dir, closed).join("\n");
        assert!(spoken.contains("4.25 m/s"), "{spoken}");
        assert!(spoken.contains("agency's value for the hour"), "{spoken}");
        let twice = write_kept_record(&dir, "record", closed + 60.0, &quakes, &volcanoes, &wind, &[], &later_bulletin, &checks);
        assert!(twice.contains("lines today"), "{twice}");
        let next_again = fs::read_to_string(dir.join("2026-11-03.jsonl")).unwrap();
        assert_eq!(next_again.matches("\"of\":\"wind\"").count(), 1, "{next_again}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn closed_rain_without_a_same_hour_value_stays_pending() {
        let dir = scratch("wait");
        let bulletin = bulletin();
        let wind = vec![WindSample {
            lat: 10.0,
            lon: 10.0,
            temp_c: 20.0,
            wind_ms: 8.0,
            from_deg: 90.0,
            precip_mm: Some(9.0),
        }];
        let opened = write_kept_record(&dir, "record", 1_700_000_000.0, &[], &[], &wind, &[], &bulletin, &[]);
        assert!(opened.contains("2023-11-14.jsonl"), "{opened}");
        let closed = geo::days_from_civil(2026, 11, 3) as f64 * 86400.0 + 9.0 * 3600.0;
        let held = write_kept_record(&dir, "record", closed, &[], &[], &wind, &[], &bulletin, &[]);
        assert!(held.contains("2026-11-03.jsonl"), "{held}");
        let next = fs::read_to_string(dir.join("2026-11-03.jsonl")).unwrap_or_default();
        assert!(!next.contains("\"k\":\"result\""), "{next}");
        assert!(!next.contains("\"later\":9"), "{next}");
        let state: Value = serde_json::from_str(&fs::read_to_string(dir.join("state.json")).unwrap()).unwrap();
        let pending = state.get("pending").and_then(|v| v.as_array()).expect("pending");
        assert!(pending.iter().any(|item| item.get("of").and_then(|v| v.as_str()) == Some("rain")), "{pending:?}");
        let spoken = look_lines(&dir, closed).join("\n");
        assert!(spoken.contains("waiting for the agency value"), "{spoken}");
        assert!(spoken.contains("rain hour 2026-11-03T07"), "{spoken}");
        let _ = fs::remove_dir_all(&dir);
    }
}
