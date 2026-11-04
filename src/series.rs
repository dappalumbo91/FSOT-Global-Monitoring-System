//! Measured series held on disk so a later pass can pull them.
//!
//! The desk quotes one line. These files keep every agency row from that same
//! download: quiet hours, gaps, and the values in between. FSOT is not scoring them.

use crate::geo;
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

pub type Row = Map<String, Value>;

#[derive(Clone, Debug)]
pub struct HeldNote {
    pub name: String,
    pub source: String,
    pub rows: usize,
    pub first: String,
    pub last: String,
    pub fresh: bool,
    pub detail: String,
}

impl HeldNote {
    pub fn line(&self) -> String {
        if self.rows == 0 && !self.fresh {
            let detail = if self.detail.is_empty() { "is not held.".to_string() } else { self.detail.clone() };
            return format!("pull {}: {detail}", self.name);
        }
        if self.rows == 0 {
            return format!("pull {}: no rows held. The catalog loaded with no rows.", self.name);
        }
        let span = if self.first.is_empty() {
            String::new()
        } else if self.first == self.last {
            format!(", {}", self.first)
        } else {
            format!(", {} through {}", self.first, self.last)
        };
        let tail = if self.fresh {
            String::new()
        } else {
            " This pass did not load. The held file is unchanged.".to_string()
        };
        format!("pull {}: {} rows{span}.{tail}", self.name, self.rows)
    }
}

#[derive(Clone, Debug)]
pub struct SeriesPull {
    pub name: String,
    pub source: String,
    pub pulled: String,
    pub rows: Vec<Row>,
}

const ORDER: &[&str] = &[
    "rtsw", "protons", "electrons", "mag", "f107", "regions", "sunspots", "dst", "kp", "imf", "xray", "flares", "scales",
    "alerts", "cme", "oni", "ao", "nao", "mjo", "sea-ice", "co2", "quakes", "vaac", "storms", "fires", "wind-grid", "wind-hours",
    "ocean-grid", "ocean-hours", "gauges", "ensemble", "air",
];

pub fn sort_notes(notes: &mut [HeldNote]) {
    notes.sort_by_key(|note| ORDER.iter().position(|name| *name == note.name).unwrap_or(ORDER.len()));
}

pub fn store(root: &Path, name: &str, source: &str, rows: Vec<Row>) -> HeldNote {
    let mut note = note_of(name, source, &rows, true, "");
    match write_file(root, name, source, &rows) {
        Ok(()) => note,
        Err(err) => {
            let mut kept = recall_or(root, name, source, &err);
            kept.detail = err;
            note = kept;
            note
        }
    }
}

pub fn recall_or(root: &Path, name: &str, source: &str, detail: &str) -> HeldNote {
    match pull(root, name) {
        Ok(pulled) => {
            let mut note = note_of(name, source, &pulled.rows, false, detail);
            if note.source.is_empty() {
                note.source = pulled.source;
            }
            note
        }
        Err(_) => HeldNote {
            name: name.to_string(),
            source: source.to_string(),
            rows: 0,
            first: String::new(),
            last: String::new(),
            fresh: false,
            detail: detail.to_string(),
        },
    }
}

pub fn pull(root: &Path, name: &str) -> Result<SeriesPull, String> {
    let path = series_path(root, name)?;
    let text = fs::read_to_string(&path).map_err(|_| format!("Series {name} is not held."))?;
    let value: Value = serde_json::from_str(&text).map_err(|_| format!("Series {name} did not parse."))?;
    let rows = value
        .get("rows")
        .and_then(|item| item.as_array())
        .ok_or_else(|| format!("Series {name} did not parse."))?
        .iter()
        .filter_map(|item| item.as_object().cloned())
        .collect();
    Ok(SeriesPull {
        name: name.to_string(),
        source: value.get("source").and_then(|item| item.as_str()).unwrap_or("").to_string(),
        pulled: value.get("pulled").and_then(|item| item.as_str()).unwrap_or("").to_string(),
        rows,
    })
}

pub fn object_rows(body: &str, time_key: &str) -> Result<Vec<Row>, ()> {
    let value: Value = serde_json::from_str(body).map_err(|_| ())?;
    let list = value.as_array().ok_or(())?;
    let mut out = Vec::new();
    for item in list {
        let Some(obj) = item.as_object() else { continue };
        let t = obj.get(time_key).and_then(|item| item.as_str()).unwrap_or("").trim().to_string();
        if t.is_empty() {
            continue;
        }
        let mut row = Row::new();
        row.insert("t".into(), Value::String(t));
        for (key, item) in obj {
            if key == time_key {
                continue;
            }
            row.insert(key.clone(), finite_or_null(item.clone()));
        }
        out.push(row);
    }
    Ok(out)
}

/// Headered SWPC table. `t` is `time_column` when that cell is text, otherwise `time_tag`.
pub fn table_rows(body: &str, time_column: &str) -> Result<Vec<Row>, ()> {
    let value: Value = serde_json::from_str(body).map_err(|_| ())?;
    let list = value.as_array().ok_or(())?;
    let header = list.first().and_then(|row| row.as_array()).ok_or(())?;
    let names: Vec<String> = header.iter().map(|cell| cell.as_str().unwrap_or("").to_string()).collect();
    let mut out = Vec::new();
    for item in list.iter().skip(1) {
        let Some(cells) = item.as_array() else { continue };
        let mut row = Row::new();
        let mut stamped = String::new();
        let mut tagged = String::new();
        for (index, name) in names.iter().enumerate() {
            if name.is_empty() {
                continue;
            }
            let cell = finite_or_null(cells.get(index).cloned().unwrap_or(Value::Null));
            if name == time_column {
                stamped = cell.as_str().unwrap_or("").trim().to_string();
            }
            if name == "time_tag" {
                tagged = cell.as_str().unwrap_or("").trim().to_string();
            }
            row.insert(name.clone(), cell);
        }
        let t = if stamped.is_empty() { tagged } else { stamped };
        if t.is_empty() {
            continue;
        }
        row.insert("t".into(), Value::String(t));
        out.push(row);
    }
    Ok(out)
}

pub fn scales_rows(body: &str) -> Result<Vec<Row>, ()> {
    let value: Value = serde_json::from_str(body).map_err(|_| ())?;
    let obj = value.as_object().ok_or(())?;
    let mut keys: Vec<&String> = obj.keys().collect();
    keys.sort_by_key(|key| key.parse::<i32>().unwrap_or(i32::MAX));
    let mut out = Vec::new();
    for key in keys {
        let Some(day) = obj.get(key) else { continue };
        let date = day.get("DateStamp").and_then(|item| item.as_str()).unwrap_or("").trim();
        let time = day.get("TimeStamp").and_then(|item| item.as_str()).unwrap_or("").trim();
        let t = format!("{date} {time}").trim().to_string();
        let mut row = Row::new();
        row.insert("t".into(), Value::String(if t.is_empty() { key.clone() } else { t }));
        row.insert("day".into(), Value::String(key.clone()));
        for band in ["R", "S", "G"] {
            let scale = day.get(band).and_then(|item| item.get("Scale"));
            let text = day.get(band).and_then(|item| item.get("Text")).and_then(|item| item.as_str()).unwrap_or("");
            row.insert(format!("{band}_scale"), scale.cloned().map(finite_or_text).unwrap_or(Value::Null));
            row.insert(format!("{band}_text"), Value::String(text.trim().to_string()));
        }
        out.push(row);
    }
    Ok(out)
}

pub fn oni_rows(body: &str) -> Result<Vec<Row>, ()> {
    let mut out = Vec::new();
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
        let Ok(total) = parts[2].parse::<f64>() else { continue };
        let Ok(anom) = parts[3].parse::<f64>() else { continue };
        if !total.is_finite() || !anom.is_finite() {
            continue;
        }
        let Some(month) = season_month(season) else { continue };
        let mut row = Row::new();
        row.insert("t".into(), Value::String(format!("{year:04}-{month:02}")));
        row.insert("season".into(), Value::String(season.to_string()));
        row.insert("year".into(), Value::from(year));
        put_f64(&mut row, "total", total);
        put_f64(&mut row, "anom", anom);
        out.push(row);
    }
    if out.is_empty() { Err(()) } else { Ok(out) }
}

pub fn monthly_rows(body: &str) -> Result<Vec<Row>, ()> {
    let mut out = Vec::new();
    for line in body.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }
        let Ok(year) = parts[0].parse::<i32>() else { continue };
        let Ok(month) = parts[1].parse::<u32>() else { continue };
        if !(1..=12).contains(&month) {
            continue;
        }
        let Ok(index) = parts[2].parse::<f64>() else { continue };
        if !index.is_finite() {
            continue;
        }
        let mut row = Row::new();
        row.insert("t".into(), Value::String(format!("{year:04}-{month:02}")));
        row.insert("year".into(), Value::from(year));
        row.insert("month".into(), Value::from(month));
        put_f64(&mut row, "index", index);
        out.push(row);
    }
    if out.is_empty() { Err(()) } else { Ok(out) }
}

pub fn donki_rows(body: &str) -> Result<Vec<Row>, ()> {
    let value: Value = serde_json::from_str(body).map_err(|_| ())?;
    let list = value.as_array().ok_or(())?;
    let mut out = Vec::new();
    for event in list {
        let t = event.get("startTime").and_then(|item| item.as_str()).unwrap_or("").trim().to_string();
        if t.is_empty() {
            continue;
        }
        let analyses = event.get("cmeAnalyses").and_then(|item| item.as_array());
        let Some(analyses) = analyses.filter(|rows| !rows.is_empty()) else {
            let mut row = Row::new();
            row.insert("t".into(), Value::String(t));
            row.insert("catalog".into(), Value::String("donki".into()));
            out.push(row);
            continue;
        };
        for analysis in analyses {
            let mut row = Row::new();
            row.insert("t".into(), Value::String(t.clone()));
            row.insert("catalog".into(), Value::String("donki".into()));
            copy_finite(&mut row, analysis, "speed", "speed");
            if let Some(kind) = analysis.get("type").and_then(|item| item.as_str()) {
                row.insert("type".into(), Value::String(kind.to_string()));
            }
            if let Some(flag) = analysis.get("isMostAccurate").and_then(|item| item.as_bool()) {
                row.insert("most_accurate".into(), Value::Bool(flag));
            }
            copy_finite(&mut row, analysis, "latitude", "latitude");
            copy_finite(&mut row, analysis, "longitude", "longitude");
            copy_finite(&mut row, analysis, "halfAngle", "half_angle");
            if let Some(note) = analysis.get("note").and_then(|item| item.as_str()) {
                if !note.is_empty() {
                    row.insert("note".into(), Value::String(note.to_string()));
                }
            }
            out.push(row);
        }
    }
    Ok(out)
}

pub fn cactus_rows(body: &str) -> Result<Vec<Row>, ()> {
    if !body.contains("# CME") {
        return Err(());
    }
    let mut out = Vec::new();
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
        let mut row = Row::new();
        row.insert("t".into(), Value::String(onset.to_string()));
        row.insert("catalog".into(), Value::String("cactus".into()));
        row.insert("id".into(), Value::String(id.to_string()));
        put_parsed(&mut row, "dt0", parts[2]);
        put_parsed(&mut row, "pa", parts[3]);
        put_parsed(&mut row, "da", parts[4]);
        put_parsed(&mut row, "v", parts[5]);
        if let Some(value) = parts.get(6) {
            put_parsed(&mut row, "dv", value);
        }
        if let Some(value) = parts.get(7) {
            put_parsed(&mut row, "minv", value);
        }
        if let Some(value) = parts.get(8) {
            put_parsed(&mut row, "maxv", value);
        }
        if let Some(halo) = parts.get(9) {
            if !halo.is_empty() {
                row.insert("halo".into(), Value::String((*halo).to_string()));
            }
        }
        out.push(row);
    }
    Ok(out)
}

pub fn hour_rows(body: &str, fields: &[(&str, &str)]) -> Vec<Row> {
    let Ok(value) = serde_json::from_str::<Value>(body) else { return Vec::new() };
    let items: Vec<&Value> = if let Some(list) = value.as_array() { list.iter().collect() } else { vec![&value] };
    let mut out = Vec::new();
    for item in items {
        let Some(lat) = item.get("latitude").and_then(|v| v.as_f64()) else { continue };
        let Some(lon) = item.get("longitude").and_then(|v| v.as_f64()) else { continue };
        let Some(hourly) = item.get("hourly") else { continue };
        let Some(times) = hourly.get("time").and_then(|v| v.as_array()) else { continue };
        for (index, stamp) in times.iter().enumerate() {
            let t = stamp.as_str().unwrap_or("").to_string();
            if t.is_empty() {
                continue;
            }
            let mut row = Row::new();
            row.insert("t".into(), Value::String(t));
            put_f64(&mut row, "lat", lat);
            put_f64(&mut row, "lon", lon);
            for (source, name) in fields {
                let cell = hourly.get(*source).and_then(|v| v.as_array()).and_then(|column| column.get(index));
                match cell.and_then(json_finite) {
                    Some(number) => put_f64(&mut row, name, number),
                    None => {
                        row.insert((*name).into(), Value::Null);
                    }
                }
            }
            out.push(row);
        }
    }
    out
}

pub fn current_rows(body: &str, fields: &[(&str, &str)]) -> Vec<Row> {
    let Ok(value) = serde_json::from_str::<Value>(body) else { return Vec::new() };
    let items: Vec<&Value> = if let Some(list) = value.as_array() { list.iter().collect() } else { vec![&value] };
    let mut out = Vec::new();
    for item in items {
        let Some(lat) = item.get("latitude").and_then(|v| v.as_f64()) else { continue };
        let Some(lon) = item.get("longitude").and_then(|v| v.as_f64()) else { continue };
        let Some(current) = item.get("current") else { continue };
        let t = current.get("time").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let mut row = Row::new();
        row.insert("t".into(), Value::String(t));
        put_f64(&mut row, "lat", lat);
        put_f64(&mut row, "lon", lon);
        for (source, name) in fields {
            match current.get(*source).and_then(json_finite) {
                Some(number) => put_f64(&mut row, name, number),
                None => {
                    row.insert((*name).into(), Value::Null);
                }
            }
        }
        out.push(row);
    }
    out
}

pub fn ensemble_rows(body: &str) -> Result<Vec<Row>, ()> {
    let value: Value = serde_json::from_str(body).map_err(|_| ())?;
    let lat = value.get("latitude").and_then(|v| v.as_f64());
    let lon = value.get("longitude").and_then(|v| v.as_f64());
    let hourly = value.get("hourly").and_then(|item| item.as_object()).ok_or(())?;
    let times = hourly.get("time").and_then(|item| item.as_array()).ok_or(())?;
    let mut keys: Vec<&String> = hourly.keys().filter(|key| key.as_str() != "time").collect();
    keys.sort();
    let mut out = Vec::new();
    for (index, stamp) in times.iter().enumerate() {
        let t = stamp.as_str().unwrap_or("").to_string();
        if t.is_empty() {
            continue;
        }
        for key in &keys {
            let cell = hourly.get(*key).and_then(|item| item.as_array()).and_then(|column| column.get(index));
            let mut row = Row::new();
            row.insert("t".into(), Value::String(t.clone()));
            if let Some(lat) = lat {
                put_f64(&mut row, "lat", lat);
            }
            if let Some(lon) = lon {
                put_f64(&mut row, "lon", lon);
            }
            row.insert("series".into(), Value::String((*key).clone()));
            match cell.and_then(json_finite) {
                Some(number) => put_f64(&mut row, "value", number),
                None => {
                    row.insert("value".into(), Value::Null);
                }
            }
            out.push(row);
        }
    }
    if out.is_empty() { Err(()) } else { Ok(out) }
}

/// NOAA GML Mauna Loa daily average. `-99.99` is the missing code. A real zero stays zero.
/// Rows from December 2022 through 4 July 2023 are the file's Maunakea substitute site.
pub fn co2_rows(body: &str) -> Result<Vec<Row>, ()> {
    let mut out = Vec::new();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cells: Vec<&str> = line.split(',').map(|cell| cell.trim()).collect();
        if cells.len() < 5 {
            continue;
        }
        let Ok(year) = cells[0].parse::<i32>() else { continue };
        let Ok(month) = cells[1].parse::<u32>() else { continue };
        let Ok(day) = cells[2].parse::<u32>() else { continue };
        if !(1800..=3000).contains(&year) || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            continue;
        }
        let t = format!("{year:04}-{month:02}-{day:02}");
        let mut row = Row::new();
        row.insert("t".into(), Value::String(t.clone()));
        row.insert("station".into(), Value::String(co2_station(&t).to_string()));
        put_optional(&mut row, "decimal", cells[3]);
        match co2_ppm(cells[4]) {
            Some(value) => put_f64(&mut row, "co2_ppm", value),
            None => {
                row.insert("co2_ppm".into(), Value::Null);
            }
        }
        out.push(row);
    }
    if out.is_empty() { Err(()) } else { Ok(out) }
}

fn co2_station(day: &str) -> &'static str {
    if day >= "2022-12-01" && day <= "2023-07-04" { "Maunakea Observatories" } else { "Mauna Loa Observatory" }
}

fn co2_ppm(text: &str) -> Option<f64> {
    let value = parse_finite(text)?;
    if value <= -99.0 { None } else { Some(value) }
}

/// UNESCO IOC active station list. One latest sample per station. -999 stays null. A real 0 stays 0.
pub fn gauge_rows(body: &str) -> Result<Vec<Row>, ()> {
    let value: Value = serde_json::from_str(body).map_err(|_| ())?;
    let list = value.as_array().ok_or(())?;
    let mut out = Vec::new();
    for item in list {
        let Some(obj) = item.as_object() else { continue };
        let Some(code) = station_text(obj, "Code") else { continue };
        let mut row = Row::new();
        row.insert("t".into(), Value::String(gauge_time(&station_text(obj, "lasttime").unwrap_or_default())));
        row.insert("code".into(), Value::String(code));
        insert_text(&mut row, "location", station_text(obj, "Location"));
        insert_text(&mut row, "country", station_text(obj, "country"));
        insert_text(&mut row, "type", station_text(obj, "type"));
        insert_text(&mut row, "sensor", station_text(obj, "sensor"));
        insert_text(&mut row, "units", station_text(obj, "units"));
        insert_text(&mut row, "connect", station_text(obj, "connect"));
        match station_number(obj, "Lat").or_else(|| station_number(obj, "lat")) {
            Some(lat) => put_f64(&mut row, "lat", lat),
            None => {
                row.insert("lat".into(), Value::Null);
            }
        }
        match station_number(obj, "Lon").or_else(|| station_number(obj, "lon")) {
            Some(lon) => put_f64(&mut row, "lon", lon),
            None => {
                row.insert("lon".into(), Value::Null);
            }
        }
        match gauge_level(station_number(obj, "lastvalue")) {
            Some(level) => put_f64(&mut row, "level", level),
            None => {
                row.insert("level".into(), Value::Null);
            }
        }
        out.push(row);
    }
    Ok(out)
}

fn station_text(obj: &Map<String, Value>, key: &str) -> Option<String> {
    obj.get(key).and_then(|value| value.as_str()).map(|text| text.trim().to_string()).filter(|text| !text.is_empty())
}

fn station_number(obj: &Map<String, Value>, key: &str) -> Option<f64> {
    let value = obj.get(key)?;
    let number = value.as_f64().or_else(|| value.as_str().and_then(|text| parse_finite(text)))?;
    number.is_finite().then_some(number)
}

fn insert_text(row: &mut Row, key: &str, text: Option<String>) {
    match text {
        Some(text) => {
            row.insert(key.into(), Value::String(text));
        }
        None => {
            row.insert(key.into(), Value::Null);
        }
    }
}

fn gauge_level(value: Option<f64>) -> Option<f64> {
    let value = value?;
    if value <= -99.0 { None } else { Some(value) }
}

fn gauge_time(raw: &str) -> String {
    let raw = raw.trim();
    if raw.len() >= 19 && raw.as_bytes().get(4) == Some(&b'-') && raw.as_bytes().get(10) == Some(&b' ') {
        return format!("{}T{}Z", &raw[..10], &raw[11..19]);
    }
    raw.to_string()
}

/// NSIDC Sea Ice Index daily row. `hemisphere` is `N` or `S`. A non-numeric extent stays null.
pub fn sea_ice_rows(body: &str, hemisphere: &str) -> Result<Vec<Row>, ()> {
    let mut out = Vec::new();
    for line in body.lines() {
        let cells: Vec<&str> = line.split(',').map(|cell| cell.trim()).collect();
        if cells.len() < 5 {
            continue;
        }
        let Ok(year) = cells[0].parse::<i32>() else { continue };
        let Ok(month) = cells[1].parse::<u32>() else { continue };
        let Ok(day) = cells[2].parse::<u32>() else { continue };
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            continue;
        }
        let mut row = Row::new();
        row.insert("t".into(), Value::String(format!("{year:04}-{month:02}-{day:02}")));
        row.insert("hemisphere".into(), Value::String(hemisphere.to_string()));
        put_optional(&mut row, "extent", cells[3]);
        put_optional(&mut row, "missing", cells[4]);
        out.push(row);
    }
    if out.is_empty() { Err(()) } else { Ok(out) }
}

/// BOM Wheeler–Hendon RMM text. Missing values 999 and 1.E36 stay null.
pub fn rmm_rows(body: &str) -> Result<Vec<Row>, ()> {
    let mut out = Vec::new();
    for line in body.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 7 {
            continue;
        }
        let Ok(year) = parts[0].parse::<i32>() else { continue };
        let Ok(month) = parts[1].parse::<u32>() else { continue };
        let Ok(day) = parts[2].parse::<u32>() else { continue };
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            continue;
        }
        let mut row = Row::new();
        row.insert("t".into(), Value::String(format!("{year:04}-{month:02}-{day:02}")));
        put_rmm(&mut row, "rmm1", parts[3]);
        put_rmm(&mut row, "rmm2", parts[4]);
        put_rmm(&mut row, "phase", parts[5]);
        put_rmm(&mut row, "amplitude", parts[6]);
        if parts.len() > 7 {
            row.insert("method".into(), Value::String(parts[7..].join(" ")));
        }
        out.push(row);
    }
    if out.is_empty() { Err(()) } else { Ok(out) }
}

/// NASA FIRMS VIIRS active-fire CSV. A low-confidence row stays. An empty catalog is valid.
pub fn firms_rows(body: &str) -> Result<Vec<Row>, ()> {
    let mut lines = body.lines();
    let header = loop {
        let Some(line) = lines.next() else { return Err(()) };
        let line = line.trim_start_matches('\u{feff}').trim();
        if !line.is_empty() {
            break line.to_string();
        }
    };
    let names = csv_cells(&header);
    let lower: Vec<String> = names.iter().map(|name| name.to_ascii_lowercase()).collect();
    if !lower.iter().any(|name| name == "latitude") || !lower.iter().any(|name| name == "frp") {
        return Err(());
    }
    let index = |name: &str| lower.iter().position(|cell| cell == name);
    let mut out = Vec::new();
    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let cells = csv_cells(line);
        let Some(lat) = index("latitude").and_then(|i| cells.get(i)).and_then(|text| parse_finite(text)) else { continue };
        let Some(lon) = index("longitude").and_then(|i| cells.get(i)).and_then(|text| parse_finite(text)) else { continue };
        let date = index("acq_date").and_then(|i| cells.get(i)).map(|text| text.as_str()).unwrap_or("");
        let clock = index("acq_time").and_then(|i| cells.get(i)).map(|text| text.as_str()).unwrap_or("");
        let mut row = Row::new();
        row.insert("t".into(), Value::String(firms_time(date, clock)));
        put_f64(&mut row, "lat", lat);
        put_f64(&mut row, "lon", lon);
        put_cell(&mut row, &cells, &index, "bright_ti4", "bright_ti4", true);
        put_cell(&mut row, &cells, &index, "bright_ti5", "bright_ti5", true);
        put_cell(&mut row, &cells, &index, "frp", "frp", true);
        put_cell(&mut row, &cells, &index, "scan", "scan", true);
        put_cell(&mut row, &cells, &index, "track", "track", true);
        put_cell(&mut row, &cells, &index, "confidence", "confidence", false);
        put_cell(&mut row, &cells, &index, "satellite", "satellite", false);
        put_cell(&mut row, &cells, &index, "daynight", "daynight", false);
        put_cell(&mut row, &cells, &index, "version", "version", false);
        out.push(row);
    }
    Ok(out)
}

fn csv_cells(line: &str) -> Vec<String> {
    line.split(',').map(|cell| cell.trim().trim_matches('"').to_string()).collect()
}

fn firms_time(date: &str, clock: &str) -> String {
    let day = if date.len() >= 10 { &date[..10] } else { date };
    let digits: String = clock.chars().filter(|ch| ch.is_ascii_digit()).collect();
    if day.len() == 10 && !digits.is_empty() {
        let padded = format!("{digits:0>4}");
        return format!("{day}T{}:{}:00Z", &padded[..2], &padded[2..4]);
    }
    day.to_string()
}

fn put_cell(row: &mut Row, cells: &[String], index: &dyn Fn(&str) -> Option<usize>, source: &str, name: &str, numeric: bool) {
    let Some(text) = index(source).and_then(|slot| cells.get(slot)) else {
        row.insert(name.into(), Value::Null);
        return;
    };
    if text.is_empty() {
        row.insert(name.into(), Value::Null);
        return;
    }
    if numeric {
        match parse_finite(text) {
            Some(number) => put_f64(row, name, number),
            None => {
                row.insert(name.into(), Value::Null);
            }
        }
    } else {
        row.insert(name.into(), Value::String(text.clone()));
    }
}

fn put_optional(row: &mut Row, key: &str, text: &str) {
    match parse_finite(text) {
        Some(number) => put_f64(row, key, number),
        None => {
            row.insert(key.into(), Value::Null);
        }
    }
}

fn put_rmm(row: &mut Row, key: &str, token: &str) {
    match rmm_number(token) {
        Some(number) => put_f64(row, key, number),
        None => {
            row.insert(key.into(), Value::Null);
        }
    }
}

/// 999 and 1.E36 are the BOM missing codes. A finite index, including a small one, stays.
fn rmm_number(token: &str) -> Option<f64> {
    let token = token.trim();
    if token.is_empty() {
        return None;
    }
    let normalized = token.replace(".E", ".0E").replace(".e", ".0e");
    let value = normalized.parse::<f64>().ok()?;
    if !value.is_finite() || value.abs() >= 1.0e30 || (value.abs() - 999.0).abs() < 1e-3 {
        return None;
    }
    Some(value)
}

fn parse_finite(text: &str) -> Option<f64> {
    let value = text.trim().parse::<f64>().ok()?;
    value.is_finite().then_some(value)
}

pub fn put_f64(row: &mut Row, key: &str, value: f64) {
    if let Some(number) = serde_json::Number::from_f64(value) {
        row.insert(key.into(), Value::Number(number));
    }
}

fn note_of(name: &str, source: &str, rows: &[Row], fresh: bool, detail: &str) -> HeldNote {
    let mut first = String::new();
    let mut last = String::new();
    for row in rows {
        let Some(t) = row.get("t").and_then(|item| item.as_str()) else { continue };
        if t.is_empty() {
            continue;
        }
        if first.is_empty() || t < first.as_str() {
            first = t.to_string();
        }
        if last.is_empty() || t > last.as_str() {
            last = t.to_string();
        }
    }
    HeldNote {
        name: name.to_string(),
        source: source.to_string(),
        rows: rows.len(),
        first,
        last,
        fresh,
        detail: detail.to_string(),
    }
}

fn write_file(root: &Path, name: &str, source: &str, rows: &[Row]) -> Result<(), String> {
    let path = series_path(root, name)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let pulled = geo::format_unix_utc(geo::unix_now());
    let body = serde_json::json!({
        "name": name,
        "source": source,
        "pulled": pulled,
        "rows": rows.iter().cloned().map(Value::Object).collect::<Vec<_>>(),
    });
    fs::write(path, serde_json::to_vec(&body).map_err(|err| err.to_string())?).map_err(|err| err.to_string())
}

fn series_path(root: &Path, name: &str) -> Result<PathBuf, String> {
    if !valid_name(name) {
        return Err(format!("Series {name} is not a pull name."));
    }
    Ok(root.join(format!("{name}.json")))
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
}

fn finite_or_null(value: Value) -> Value {
    match value {
        Value::Number(number) => match number.as_f64() {
            Some(value) if value.is_finite() => Value::Number(number),
            _ => Value::Null,
        },
        other => other,
    }
}

fn finite_or_text(value: Value) -> Value {
    if let Some(text) = value.as_str() {
        return Value::String(text.trim().to_string());
    }
    if let Some(number) = value.as_i64() {
        return Value::String(number.to_string());
    }
    if let Some(number) = value.as_f64() {
        if number.is_finite() {
            return Value::String(format!("{number}"));
        }
    }
    Value::Null
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

fn copy_finite(row: &mut Row, source: &Value, from: &str, to: &str) {
    if let Some(number) = source.get(from).and_then(json_finite) {
        put_f64(row, to, number);
    }
}

fn put_parsed(row: &mut Row, key: &str, text: &str) {
    let Ok(number) = text.parse::<f64>() else {
        row.insert(key.into(), Value::Null);
        return;
    };
    if number.is_finite() && number >= 0.0 {
        put_f64(row, key, number);
    } else {
        row.insert(key.into(), Value::Null);
    }
}

fn season_month(season: &str) -> Option<u32> {
    Some(match season {
        "JFM" => 1,
        "FMA" => 2,
        "MAM" => 3,
        "AMJ" => 4,
        "MJJ" => 5,
        "JJA" => 6,
        "JAS" => 7,
        "ASO" => 8,
        "SON" => 9,
        "OND" => 10,
        "NDJ" => 11,
        "DJF" => 12,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fsot-series-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn series_keep_quiet_rows_and_pull_returns_them() {
        let dir = scratch("rows");
        let rtsw = object_rows(
            r#"[{"time_tag":"2026-11-03T03:16:00","active":true,"source":"SOLAR1","proton_speed":394.3,"proton_density":1.78,"proton_temperature":74806,"proton_vx_gse":null},{"time_tag":"2026-11-03T03:20:00","active":false,"source":"TAIL","proton_speed":10,"proton_density":null,"proton_temperature":1}]"#,
            "time_tag",
        )
        .unwrap();
        assert_eq!(rtsw.len(), 2);
        assert_eq!(rtsw[1].get("active").and_then(|v| v.as_bool()), Some(false));
        assert!(rtsw[1].get("proton_density").unwrap().is_null());
        assert!(rtsw[0].get("proton_vx_gse").unwrap().is_null());
        let note = store(&dir, "rtsw", "NOAA SWPC RTSW", rtsw);
        assert!(note.line().contains("2 rows"), "{}", note.line());
        let pulled = pull(&dir, "rtsw").unwrap();
        assert_eq!(pulled.name, "rtsw");
        assert!(!pulled.pulled.is_empty());
        assert_eq!(pulled.source, "NOAA SWPC RTSW");
        assert_eq!(pulled.rows.len(), 2);
        assert_eq!(pulled.rows[0].get("proton_speed").and_then(|v| v.as_f64()), Some(394.3));
        let kept = recall_or(&dir, "rtsw", "NOAA SWPC RTSW", "NOAA SWPC RTSW plasma did not load.");
        assert!(!kept.fresh);
        assert!(kept.line().contains("unchanged"), "{}", kept.line());
        assert!(pull(&dir, "../rtsw").is_err());

        let protons = object_rows(
            r#"[{"time_tag":"2026-11-03T03:00:00Z","energy":">=10 MeV","flux":0.1},{"time_tag":"2026-11-03T03:10:00Z","energy":">=60 MeV","flux":0.224}]"#,
            "time_tag",
        )
        .unwrap();
        assert_eq!(protons.len(), 2);
        assert_eq!(protons[1].get("energy").and_then(|v| v.as_str()), Some(">=60 MeV"));

        let oni = oni_rows("SEAS  YR   TOTAL   ANOM\nMJJ 2026  29.02   1.39\nJAS 2026  29.12   2.16\n").unwrap();
        assert_eq!(oni.len(), 2);
        assert_eq!(oni[0].get("t").and_then(|v| v.as_str()), Some("2026-05"));
        assert_eq!(oni[1].get("anom").and_then(|v| v.as_f64()), Some(2.16));
        let ao = monthly_rows("2026    8   -0.0560\n2026    9    1.1556\n").unwrap();
        assert_eq!(ao.len(), 2);
        assert_eq!(ao[0].get("index").and_then(|v| v.as_f64()).map(|v| (v * 10000.0).round()), Some(-560.0));

        let kp = object_rows(
            r#"[{"time_tag":"2026-11-03T00:00:00","kp":1.0,"observed":"observed","noaa_scale":null},{"time_tag":"2026-11-03T06:00:00","kp":2.67,"observed":"predicted","noaa_scale":null}]"#,
            "time_tag",
        )
        .unwrap();
        assert_eq!(kp.len(), 2);
        assert_eq!(kp[0].get("kp").and_then(|v| v.as_f64()), Some(1.0));
        assert_eq!(kp[1].get("observed").and_then(|v| v.as_str()), Some("predicted"));

        let imf = table_rows(
            r#"[["time_tag","bt","bz","propagated_time_tag"],["2026-11-03T01:00:00Z",1.0,-0.2,"2026-11-03T02:00:00Z"],["2026-11-03T01:01:00Z",4.0,0.1,"2026-11-03T02:01:00Z"]]"#,
            "propagated_time_tag",
        )
        .unwrap();
        assert_eq!(imf.len(), 2);
        assert_eq!(imf[0].get("t").and_then(|v| v.as_str()), Some("2026-11-03T02:00:00Z"));
        assert_eq!(imf[1].get("bt").and_then(|v| v.as_f64()), Some(4.0));

        let scales = scales_rows(
            r#"{"0":{"DateStamp":"2026-11-03","TimeStamp":"03:19","R":{"Scale":0,"Text":"none"},"S":{"Scale":"0","Text":"none"},"G":{"Scale":"0","Text":"none"}},"1":{"DateStamp":"2026-11-03","TimeStamp":"12:00","R":{"Scale":"1","Text":"minor"},"S":{"Scale":"0","Text":"none"},"G":{"Scale":"0","Text":"none"}}}"#,
        )
        .unwrap();
        assert_eq!(scales.len(), 2);
        assert_eq!(scales[1].get("R_text").and_then(|v| v.as_str()), Some("minor"));
        assert_eq!(scales[0].get("R_scale").and_then(|v| v.as_str()), Some("0"));

        let cactus = cactus_rows("# CME | t0 | dt0| pa | da | v | dv | minv| maxv| halo?\n0011|2026/11/01 23:48| 05 | 041| 016| 0364| 0064| 0253| 0464|\n0012|2026/11/02 00:36| 01 | 091| 008| 0151| 0066| 0118| 0287|\n# Flow| t0 | dt0| pa | da | v\n0002|2026/11/02 06:24| 02 | 032| 006| 0496|\n").unwrap();
        assert_eq!(cactus.len(), 2);
        assert_eq!(cactus[0].get("v").and_then(|v| v.as_f64()), Some(364.0));
        assert!(cactus.iter().all(|row| row.get("v").and_then(|v| v.as_f64()) != Some(496.0)));

        let hours = hour_rows(
            r#"[{"latitude":1,"longitude":2,"hourly":{"time":["2026-11-03T01:00","2026-11-03T02:00"],"wind_speed_10m":[0.0,4.5],"precipitation":[0.0,null]}}]"#,
            &[("wind_speed_10m", "wind_ms"), ("precipitation", "precip_mm")],
        );
        assert_eq!(hours.len(), 2);
        assert_eq!(hours[0].get("wind_ms").and_then(|v| v.as_f64()), Some(0.0));
        assert_eq!(hours[0].get("precip_mm").and_then(|v| v.as_f64()), Some(0.0));
        assert!(hours[1].get("precip_mm").unwrap().is_null());

        let ensemble = ensemble_rows(
            r#"{"latitude":39.7,"longitude":-105.0,"hourly":{"time":["2026-11-03T01:00","2026-11-03T08:00"],"temperature_2m":[1,1],"temperature_2m_member01":[4,null],"precipitation_member01":[0,0.2]}}"#,
        )
        .unwrap();
        assert_eq!(ensemble.len(), 6);
        assert!(ensemble.iter().any(|row| row.get("series").and_then(|v| v.as_str()) == Some("temperature_2m_member01") && row.get("value").unwrap().is_null()));
        assert!(ensemble.iter().any(|row| row.get("t").and_then(|v| v.as_str()) == Some("2026-11-03T08:00") && row.get("series").and_then(|v| v.as_str()) == Some("precipitation_member01")));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn next_measurements_keep_a_real_zero_and_a_missing_code() {
        let fields = [
            ("pressure_msl", "pressure_hpa"),
            ("relative_humidity_2m", "humidity_pct"),
            ("wind_speed_10m", "wind_ms"),
        ];
        let current = current_rows(
            r#"[{"latitude":10,"longitude":20,"current":{"time":"2026-11-03T03:00","pressure_msl":0,"relative_humidity_2m":null,"wind_speed_10m":1.5}},{"latitude":-10,"longitude":40,"current":{"time":"2026-11-03T03:00","pressure_msl":null,"relative_humidity_2m":0,"wind_speed_10m":0}}]"#,
            &fields,
        );
        assert_eq!(current.len(), 2);
        assert_eq!(current[0].get("pressure_hpa").and_then(|v| v.as_f64()), Some(0.0));
        assert!(current[0].get("humidity_pct").unwrap().is_null());
        assert!(current[1].get("pressure_hpa").unwrap().is_null());
        assert_eq!(current[1].get("humidity_pct").and_then(|v| v.as_f64()), Some(0.0));
        let hours = hour_rows(
            r#"{"latitude":1,"longitude":2,"hourly":{"time":["2026-11-03T01:00","2026-11-03T02:00"],"pressure_msl":[0,null],"relative_humidity_2m":[null,0],"wind_speed_10m":[3,4]}}"#,
            &fields,
        );
        assert_eq!(hours.len(), 2);
        assert_eq!(hours[0].get("pressure_hpa").and_then(|v| v.as_f64()), Some(0.0));
        assert!(hours[0].get("humidity_pct").unwrap().is_null());
        assert!(hours[1].get("pressure_hpa").unwrap().is_null());
        assert_eq!(hours[1].get("humidity_pct").and_then(|v| v.as_f64()), Some(0.0));

        let ice = sea_ice_rows(
            "Year, Month, Day, Extent, Missing, Source Data\nYYYY, MM, DD, 10^6 sq km, 10^6 sq km, Source\n1978, 10, 26, 10.231, 0.000, source\n2026, 11, 1, , 0.000, source\n2026, 11, 2, 0.000, NA, source\n",
            "N",
        )
        .unwrap();
        assert_eq!(ice.len(), 3);
        assert_eq!(ice[0].get("extent").and_then(|v| v.as_f64()), Some(10.231));
        assert_eq!(ice[0].get("missing").and_then(|v| v.as_f64()), Some(0.0));
        assert_eq!(ice[0].get("hemisphere").and_then(|v| v.as_str()), Some("N"));
        assert!(ice[1].get("extent").unwrap().is_null());
        assert_eq!(ice[2].get("extent").and_then(|v| v.as_f64()), Some(0.0));
        assert!(ice[2].get("missing").unwrap().is_null());
        assert_eq!(ice[2].get("t").and_then(|v| v.as_str()), Some("2026-11-02"));

        let rmm = rmm_rows(
            "year month day RMM1 RMM2 phase amplitude\nMissing Value= 1.E36 or 999\n1974 6 1 999 1.E36 999 999\n2024 2 24 -3.14564817E-03 0.12845999 7 0.12849849 Gottschalk10_method\n",
        )
        .unwrap();
        assert_eq!(rmm.len(), 2);
        assert!(rmm[0].get("rmm1").unwrap().is_null());
        assert!(rmm[0].get("rmm2").unwrap().is_null());
        assert!(rmm[0].get("phase").unwrap().is_null());
        assert!(rmm[0].get("amplitude").unwrap().is_null());
        assert!((rmm[1].get("rmm1").and_then(|v| v.as_f64()).unwrap() + 0.00314564817).abs() < 1e-6);
        assert_eq!(rmm[1].get("phase").and_then(|v| v.as_f64()), Some(7.0));
        assert!((rmm[1].get("amplitude").and_then(|v| v.as_f64()).unwrap() - 0.12849849).abs() < 1e-6);
        assert_eq!(rmm[1].get("t").and_then(|v| v.as_str()), Some("2024-02-24"));

        let fires = firms_rows(
            "latitude,longitude,bright_ti4,scan,track,acq_date,acq_time,satellite,confidence,version,bright_ti5,frp,daynight\n-10.5,20.25,300.1,0.4,0.4,2026-11-03,456,N,low,2.0,290.0,1.5,N\n1.0,2.0,340.0,0.4,0.4,2026-11-03,1205,N20,nominal,2.0,300.0,0,D\n",
        )
        .unwrap();
        assert_eq!(fires.len(), 2);
        assert_eq!(fires[0].get("confidence").and_then(|v| v.as_str()), Some("low"));
        assert_eq!(fires[0].get("t").and_then(|v| v.as_str()), Some("2026-11-03T04:56:00Z"));
        assert_eq!(fires[1].get("frp").and_then(|v| v.as_f64()), Some(0.0));
        assert_eq!(fires[1].get("confidence").and_then(|v| v.as_str()), Some("nominal"));
        let empty = firms_rows("latitude,longitude,bright_ti4,scan,track,acq_date,acq_time,satellite,confidence,version,bright_ti5,frp,daynight\n").unwrap();
        assert!(empty.is_empty());
        assert!(firms_rows("not a fire file").is_err());

        let electrons = object_rows(
            r#"[{"time_tag":"2026-11-03T03:00:00Z","energy":">=2 MeV","flux":10},{"time_tag":"2026-11-03T04:00:00Z","energy":">=1 MeV","flux":9},{"time_tag":"2026-11-03T04:00:00Z","energy":">=2 MeV","flux":1153.1}]"#,
            "time_tag",
        )
        .unwrap();
        assert_eq!(electrons.len(), 3);
        assert!(electrons.iter().any(|row| row.get("energy").and_then(|v| v.as_str()) == Some(">=1 MeV")));
        assert!(electrons.iter().any(|row| row.get("energy").and_then(|v| v.as_str()) == Some(">=2 MeV") && row.get("flux").and_then(|v| v.as_f64()) == Some(1153.1)));

        let dir = scratch("alerts-empty");
        let note = store(&dir, "alerts", "NOAA SWPC alerts", object_rows("[]", "issue_datetime").unwrap());
        assert!(note.fresh);
        assert_eq!(note.rows, 0);
        assert!(note.line().contains("catalog loaded with no rows"), "{}", note.line());
        let _ = fs::remove_dir_all(&dir);

        let co2 = co2_rows(
            "# NOAA GML\nyear,month,day,decimal,average\n1974,5,19,1974.3781,333.46\n2022,12,1,2022.9151,-99.99\n2023,7,4,2023.5041,424.10\n2023,7,5,2023.5068,0\n",
        )
        .unwrap();
        assert_eq!(co2.len(), 4);
        assert_eq!(co2[0].get("co2_ppm").and_then(|v| v.as_f64()), Some(333.46));
        assert_eq!(co2[0].get("station").and_then(|v| v.as_str()), Some("Mauna Loa Observatory"));
        assert!(co2[1].get("co2_ppm").unwrap().is_null());
        assert_eq!(co2[1].get("station").and_then(|v| v.as_str()), Some("Maunakea Observatories"));
        assert_eq!(co2[2].get("t").and_then(|v| v.as_str()), Some("2023-07-04"));
        assert_eq!(co2[2].get("station").and_then(|v| v.as_str()), Some("Maunakea Observatories"));
        assert_eq!(co2[3].get("co2_ppm").and_then(|v| v.as_f64()), Some(0.0));
        assert_eq!(co2[3].get("station").and_then(|v| v.as_str()), Some("Mauna Loa Observatory"));
        assert!(co2_rows("# only comments\n").is_err());

        let mag = object_rows(
            r#"[{"time_tag":"2026-11-03T05:14:00Z","satellite":19,"He":0,"Hp":63.1,"Hn":null,"total":74.3,"arcjet_flag":false}]"#,
            "time_tag",
        )
        .unwrap();
        assert_eq!(mag[0].get("He").and_then(|v| v.as_f64()), Some(0.0));
        assert!(mag[0].get("Hn").unwrap().is_null());
        assert_eq!(mag[0].get("total").and_then(|v| v.as_f64()), Some(74.3));
        assert_eq!(mag[0].get("arcjet_flag").and_then(|v| v.as_bool()), Some(false));

        let spots = object_rows(
            r#"[{"time_tag":"2026-11-03T02:15:00","Region":4564,"Area":0,"Numspot":1,"Spotclass":"Axx"}]"#,
            "time_tag",
        )
        .unwrap();
        assert_eq!(spots[0].get("Area").and_then(|v| v.as_f64()), Some(0.0));
        assert_eq!(spots[0].get("Numspot").and_then(|v| v.as_i64()), Some(1));

        let gauges = gauge_rows(
            r#"[
                {"Code":"aarh","Location":"Aarhus","country":"DMK","type":"SF","Lat":56.15,"Lon":10.22,"sensor":"rad","units":"M","connect":"SF","lasttime":"2026-11-03 05:30:00.000","lastvalue":0.5},
                {"Code":"acnj2","Location":"Atlantic City","type":"SL","Lat":39.3,"Lon":-74.4,"sensor":"pwl","units":"M","lasttime":"2026-11-03 05:33:00.000","lastvalue":-7.5},
                {"Code":"zero","Location":"Zero","type":"SL","Lat":1,"Lon":2,"sensor":"rad","units":"M","lasttime":"2026-11-03 05:00:00.000","lastvalue":0},
                {"Code":"miss","Location":"Missing","type":"SL","Lat":1,"Lon":2,"sensor":"rad","units":"M","lasttime":"2026-11-03 05:00:00.000","lastvalue":-999},
                {"Code":"dkwa","Location":"DART ESE of Saipan","type":"SD","Lat":15,"Lon":147,"sensor":"prt","units":"M","lasttime":"2026-11-03 00:00:00.000","lastvalue":5604},
                {"Code":"bat","Location":"Battery","type":"SL","Lat":21,"Lon":-157,"sensor":"bat","units":"V","lasttime":"2026-11-03 05:40:00.000","lastvalue":0}
            ]"#,
        )
        .unwrap();
        assert_eq!(gauges.len(), 6);
        assert_eq!(gauges[0].get("t").and_then(|v| v.as_str()), Some("2026-11-03T05:30:00Z"));
        assert_eq!(gauges[0].get("level").and_then(|v| v.as_f64()), Some(0.5));
        assert_eq!(gauges[1].get("level").and_then(|v| v.as_f64()), Some(-7.5));
        assert_eq!(gauges[2].get("level").and_then(|v| v.as_f64()), Some(0.0));
        assert!(gauges[3].get("level").unwrap().is_null());
        assert_eq!(gauges[4].get("type").and_then(|v| v.as_str()), Some("SD"));
        assert_eq!(gauges[4].get("level").and_then(|v| v.as_f64()), Some(5604.0));
        assert_eq!(gauges[5].get("units").and_then(|v| v.as_str()), Some("V"));
        assert_eq!(gauges[5].get("level").and_then(|v| v.as_f64()), Some(0.0));
        assert!(gauge_rows("[]").unwrap().is_empty());
        assert!(gauge_rows("{").is_err());
    }
}
