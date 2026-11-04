#!/usr/bin/env python3
"""Score the pinned FSOT engine beside the monitor cache.

Reads cache/series and cache/record. Writes only FSOT/side/.
Does not touch src/, the release binary, or the monitor's cache files.
"""

from __future__ import annotations

import hashlib
import json
import os
import sys
import time
from datetime import datetime, timedelta, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SERIES = ROOT / "cache" / "series"
RECORD = ROOT / "cache" / "record"
SIDE = ROOT / "FSOT" / "side"
LEAN_VENDOR = Path(os.environ.get("FSOT_LEAN_VENDOR", r"C:\Users\damia\Desktop\FSOT-2.1-Lean\vendor"))
PIN = "AEB2ADAD6E80F487772C5DF90A2E3DDA71624AB831A6A83B94AB471AC9AAC170"


def read_json(path: Path) -> dict:
    last = None
    for attempt in range(5):
        try:
            text = path.read_text(encoding="utf-8")
            if not text.strip():
                raise json.JSONDecodeError("empty", text, 0)
            return json.loads(text)
        except json.JSONDecodeError as err:
            last = err
            if attempt < 4:
                time.sleep(0.3)
    raise RuntimeError(f"{path} did not parse: {last}")


def as_utc(text: object) -> datetime | None:
    if not isinstance(text, str) or not text.strip():
        return None
    stamp = text.strip()
    if stamp.endswith("Z"):
        stamp = stamp[:-1] + "+00:00"
    try:
        parsed = datetime.fromisoformat(stamp)
    except ValueError:
        return None
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=timezone.utc)
    return parsed.astimezone(timezone.utc)


def finite(value: object) -> float | None:
    if isinstance(value, bool) or value is None:
        return None
    try:
        number = float(value)
    except (TypeError, ValueError):
        return None
    if number != number or number in (float("inf"), float("-inf")):
        return None
    return number


def load_series(name: str) -> dict:
    data = read_json(SERIES / f"{name}.json")
    rows = data.get("rows")
    if not isinstance(rows, list):
        raise RuntimeError(f"{name} has no rows array")
    return data


def pin_ok() -> str:
    digest = hashlib.sha256((LEAN_VENDOR / "fsot_compute.py").read_bytes()).hexdigest().upper()
    if digest != PIN:
        raise SystemExit(f"pin mismatch: {digest}")
    return digest


def closed_results() -> list[dict]:
    found = []
    for path in sorted(RECORD.glob("*.jsonl")):
        for line in path.read_text(encoding="utf-8").splitlines():
            if '"k"' not in line or "result" not in line:
                continue
            try:
                row = json.loads(line)
            except json.JSONDecodeError:
                continue
            if row.get("k") == "result":
                row["_file"] = path.name
                found.append(row)
    return found


def pending_forecasts() -> list[dict]:
    state_path = RECORD / "state.json"
    if not state_path.exists():
        return []
    state = read_json(state_path)
    pending = state.get("pending")
    return pending if isinstance(pending, list) else []


def weather_inputs(rows: list[dict]) -> tuple[list[dict], dict]:
    buoys = []
    skipped = {"null_pressure": 0, "null_wind": 0, "non_positive_pressure": 0}
    for row in rows:
        pres = finite(row.get("pressure_hpa"))
        wind = finite(row.get("wind_ms"))
        lat = finite(row.get("lat"))
        lon = finite(row.get("lon"))
        if pres is None:
            skipped["null_pressure"] += 1
            continue
        if wind is None:
            skipped["null_wind"] += 1
            continue
        if lat is None or lon is None:
            continue
        if pres <= 0.0:
            skipped["non_positive_pressure"] += 1
            continue
        buoys.append(
            {
                "pres": pres,
                "gst": wind,
                "lat": lat,
                "lon": lon,
                "buoy_id": f"{lat:.0f},{lon:.0f}",
                "station": "open-meteo-grid",
            }
        )
    return buoys, skipped


def kp_inputs(rows: list[dict], issued: datetime) -> tuple[list[dict], list[dict], dict]:
    measured = []
    predicted_ahead = []
    counts = {"null_kp": 0, "unparsed_time": 0, "other_status": 0, "predicted_behind": 0}
    for row in rows:
        stamp = as_utc(row.get("t"))
        value = finite(row.get("kp"))
        status = str(row.get("observed") or "").strip().lower()
        if stamp is None:
            counts["unparsed_time"] += 1
            continue
        if value is None:
            counts["null_kp"] += 1
            continue
        item = {"t": stamp, "kp": value, "observed": status}
        if status in {"observed", "estimated"} and stamp <= issued:
            measured.append(item)
        elif status == "predicted" and stamp > issued:
            predicted_ahead.append(item)
        elif status == "predicted":
            counts["predicted_behind"] += 1
        else:
            counts["other_status"] += 1
    measured.sort(key=lambda item: item["t"])
    predicted_ahead.sort(key=lambda item: item["t"])
    return measured, predicted_ahead, counts


def quake_inputs(rows: list[dict]) -> tuple[list[dict], dict]:
    events = []
    counts = {"skipped": 0}
    for row in rows:
        stamp = as_utc(row.get("t"))
        lat = finite(row.get("lat"))
        lon = finite(row.get("lon"))
        mag = finite(row.get("mag"))
        if stamp is None or lat is None or lon is None or mag is None:
            counts["skipped"] += 1
            continue
        events.append(
            {
                "lat": lat,
                "lon": lon,
                "mag": mag,
                "time": int(stamp.timestamp() * 1000),
                "place": str(row.get("place") or ""),
                "t": stamp.strftime("%Y-%m-%dT%H:%M:%SZ"),
            }
        )
    return events, counts


def same_point(row: dict, lat: float, lon: float) -> bool:
    rlat = finite(row.get("lat"))
    rlon = finite(row.get("lon"))
    return rlat is not None and rlon is not None and abs(rlat - lat) < 0.05 and abs(rlon - lon) < 0.05


def agency_at_point(hours: list[dict], lat: float, lon: float, start: datetime, end: datetime) -> dict:
    window = []
    for row in hours:
        if not same_point(row, lat, lon):
            continue
        stamp = as_utc(row.get("t"))
        if stamp is None or stamp < start or stamp >= end:
            continue
        window.append((stamp, row))
    pressures = [finite(row.get("pressure_hpa")) for _, row in window]
    winds = [finite(row.get("wind_ms")) for _, row in window]
    rains = [finite(row.get("precip_mm")) for _, row in window]
    pressures = [v for v in pressures if v is not None]
    winds = [v for v in winds if v is not None]
    rains = [v for v in rains if v is not None]
    return {
        "hours_in_window": len(window),
        "min_pressure_hpa": min(pressures) if pressures else None,
        "max_wind_ms": max(winds) if winds else None,
        "precip_mm_sum": round(sum(rains), 3) if rains else None,
        "source": "open-meteo wind-hours",
        "role": "agency hourly forecast stored beside this cell; not a score",
    }


def round_card(fc: dict) -> dict:
    pred = dict(fc.get("predicted") or {})
    pred.pop("potentials", None)
    loc = dict(fc.get("location") or {})
    return {
        "id": fc.get("id"),
        "kind": fc.get("kind"),
        "issued_at": fc.get("issued_at"),
        "valid_from": fc.get("valid_from"),
        "valid_to": fc.get("valid_to"),
        "location": loc,
        "class": pred.get("class"),
        "valve_state": pred.get("valve_state"),
        "kill_if": fc.get("kill_if"),
        "measured_inputs": {
            "pres_now_hpa": pred.get("pres_now"),
            "wind_10m_in_gust_slot_ms": pred.get("gst_now"),
            "kp_now": pred.get("kp_now"),
            "max_mag_recent": pred.get("max_mag_recent"),
            "n_recent": pred.get("n_recent"),
        },
        "prediction": {
            "pres_max_hpa": pred.get("pres_max_hpa"),
            "gst_min_ms": pred.get("gst_min_ms"),
            "expect_kp_ge_5": pred.get("expect_kp_ge_5"),
            "expect_event": pred.get("expect_event"),
            "mag_min": pred.get("mag_min"),
            "min_count": pred.get("min_count"),
            "fsot_pressure": pred.get("fsot_pressure"),
        },
    }


def fmt(value: object, digits: int = 1) -> str:
    number = finite(value)
    if number is None:
        return "—"
    return f"{number:.{digits}f}"


def write_scorecard(path: Path, report: dict) -> None:
    lines = [
        "# Side-by-side accuracy",
        "",
        f"Issued {report['issued_at']}.",
        f"Engine pin AEB2AD. SHA-256 `{report['pin']}`.",
        "This file is the background track. The live monitor was not hooked up and its cache was not written.",
        "",
        "## Confirmed held-out",
        "",
        "Confirmed held-out accuracy is 0.",
        "An FSOT number counts only after it is written before the window closes and a later agency value arrives.",
        "The statements in this file are the first open issuances. Their windows are still ahead.",
        "",
        "## Closed hours on the monitor",
        "",
    ]
    results = report["closed_results"]
    if not results:
        lines.append("The record has no closed result line.")
    for row in results:
        forecast = finite(row.get("forecast"))
        later = finite(row.get("later"))
        gap = None if forecast is None or later is None else abs(later - forecast)
        lines.append(
            f"- `{row.get('of')}` hour `{row.get('hour')}`: stored forecast {fmt(forecast)} , "
            f"later value {fmt(later)} , absolute difference {fmt(gap)} . "
            f"Source `{row.get('src')}`. Written `{row.get('t')}`."
        )
    rain = report["closed_rain"]
    lines.extend(
        [
            "",
            f"On rain hour `{rain['hour']}`, both numbers are Open-Meteo. "
            f"The stored forecast is {fmt(rain['forecast'])} mm and the later value is {fmt(rain['later'])} mm. "
            f"The absolute difference is {fmt(rain['abs_mm'])} mm.",
            "FSOT issued no number for that hour. The FSOT error on that hour is empty.",
            "The result line does not carry a latitude or longitude, so this card does not add one.",
            "",
            "## What the engine issued from the same cache",
            "",
            "The pinned Earth-fluid functions emit a sector or a magnitude class. They do not emit a rain millimeter, a wave height, or a wind-speed hour.",
            "`pres_now`, the wind value in the gust slot, and `kp_now` are measured inputs copied onto the card.",
            "The wind value is Open-Meteo 10 m wind speed in m/s, passed through the function's gust field.",
            "Weather cards keep the function's NDBC wording. The points are Open-Meteo grid samples.",
            "",
            "### Weather, next 24 hours",
            "",
            f"Grid rows {report['weather']['grid_rows']}. Usable points {report['weather']['usable']}. "
            f"Storm points {report['weather']['storm_points']}. Quiet points {report['weather']['quiet_points']}. "
            f"Points outside both envelopes {report['weather']['gap_points']}.",
            f"Cards issued: {len(report['weather']['cards'])}.",
            "",
        ]
    )
    for card in report["weather"]["cards"]:
        loc = card["location"]
        agency = card["agency_look_ahead"]
        pred = card["prediction"]
        meas = card["measured_inputs"]
        lines.append(
            f"- `{card['id']}` {card['class']} at {fmt(loc.get('lat'), 0)}°, {fmt(loc.get('lon'), 0)}° "
            f"basin `{loc.get('basin')}`. Window through `{card['valid_to']}`."
        )
        lines.append(
            f"  Measured input: pressure {fmt(meas.get('pres_now_hpa'))} hPa, "
            f"10 m wind {fmt(meas.get('wind_10m_in_gust_slot_ms'))} m/s."
        )
        lines.append(
            f"  Class hold: pressure cap {fmt(pred.get('pres_max_hpa'), 0)} hPa, "
            f"gust floor {fmt(pred.get('gst_min_ms'), 0)} m/s. {card['kill_if']}"
        )
        lines.append(
            f"  Open-Meteo hours at this point inside the window: {agency['hours_in_window']}. "
            f"Lowest stored pressure {fmt(agency.get('min_pressure_hpa'))} hPa. "
            f"Highest stored wind {fmt(agency.get('max_wind_ms'))} m/s. "
            f"Sum of stored hourly precipitation {fmt(agency.get('precip_mm_sum'), 2)} mm. "
            "Those three figures are the agency forecast. They are not a score."
        )
    solar = report["solar"]
    lines.extend(
        [
            "",
            "### Planetary Kp, next 72 hours",
            "",
            f"Measured Kp rows at or before issuance: {solar['measured_rows']}. "
            f"Latest measured Kp {fmt(solar.get('kp_now'), 2)} at `{solar.get('kp_now_t')}`.",
            f"Null Kp rows left out: {solar['null_kp']}. Predicted rows left out of the input: "
            f"{solar['predicted_ahead_rows'] + solar['predicted_behind']}.",
            f"Card: `{solar['card']['id']}` {solar['card']['class']}. "
            f"Expect Kp ≥ 5: {solar['card']['prediction'].get('expect_kp_ge_5')}.",
            f"Kill: {solar['card']['kill_if']}",
            f"NOAA predicted Kp inside the same 72 hours peaks at {fmt(solar.get('agency_peak_kp'), 2)} "
            f"on `{solar.get('agency_peak_t')}`. That peak is the agency forecast. It is not a score.",
            "",
            "### Earthquakes, calendar window from the engine",
            "",
            f"USGS rows used: {report['quakes']['used']}. Skipped: {report['quakes']['skipped']}. "
            f"Catalog span `{report['quakes']['first_t']}` through `{report['quakes']['last_t']}`.",
            f"Horizon {report['quakes']['horizon_days']} days. Cell radius {fmt(report['quakes']['kernel_km'], 1)} km.",
            "The series is the USGS day feed. The engine's prior half-window is mostly empty on a one-day catalog, so a loading label can come from that short span.",
            f"Cards issued: {len(report['quakes']['cards'])}.",
            "",
        ]
    )
    for card in report["quakes"]["cards"]:
        loc = card["location"]
        pred = card["prediction"]
        meas = card["measured_inputs"]
        lines.append(
            f"- `{card['id']}` {card['valve_state']} near {loc.get('name')}. "
            f"{fmt(loc.get('lat'), 2)}°, {fmt(loc.get('lon'), 2)}°, radius {fmt(loc.get('radius_km'), 1)} km. "
            f"Expect an event: {pred.get('expect_event')}. Magnitude floor {fmt(pred.get('mag_min'), 1)}. "
            f"Recent count {meas.get('n_recent')}, recent max M{fmt(meas.get('max_mag_recent'), 1)}."
        )
        lines.append(f"  {card['kill_if']}")
    lines.extend(
        [
            "",
            "### Volcanic class",
            "",
            f"Cards issued: {len(report['volcanic'])}. The function keeps a USGS row when the place text contains “volcan” or the event type is volcanic. This day feed has no separate type field.",
            "",
            "## Agency forecasts still open on the monitor",
            "",
            "These rows are the monitor's stored look-ahead. They are not FSOT numbers.",
            "",
        ]
    )
    if not report["pending"]:
        lines.append("The monitor index has no pending forecast row.")
    for row in report["pending"]:
        lines.append(
            f"- `{row.get('of')}` hour `{row.get('hour')}` value {fmt(row.get('value'), 2)} "
            f"at {fmt(row.get('lat'), 2)}°, {fmt(row.get('lon'), 2)}°."
        )
    lines.extend(
        [
            "",
            "## Series read",
            "",
            f"- wind-grid pulled `{report['pulled']['wind-grid']}` source `{report['sources']['wind-grid']}`.",
            f"- wind-hours pulled `{report['pulled']['wind-hours']}` source `{report['sources']['wind-hours']}`.",
            f"- kp pulled `{report['pulled']['kp']}` source `{report['sources']['kp']}`.",
            f"- quakes pulled `{report['pulled']['quakes']}` source `{report['sources']['quakes']}`.",
            "",
            "Full engine cards, including fold fields, are in the issuance JSON beside this file.",
            "",
        ]
    )
    path.write_text("\n".join(lines), encoding="utf-8")


def main() -> None:
    digest = pin_ok()
    sys.path.insert(0, str(LEAN_VENDOR))
    import fsot_earth_fluid_forecast as earth

    issued = datetime.now(timezone.utc).replace(microsecond=0)
    wind_grid = load_series("wind-grid")
    wind_hours = load_series("wind-hours")
    kp = load_series("kp")
    quakes = load_series("quakes")

    buoys, weather_skip = weather_inputs(wind_grid["rows"])
    storm_points = sum(1 for b in buoys if b["pres"] < 1000.0 or b["gst"] >= 15.0)
    quiet_points = sum(1 for b in buoys if b["pres"] >= 1010.0 and b["gst"] < 8.0)
    weather_cards = earth.weather_forecasts(buoys, issued=issued)
    for card in weather_cards:
        loc = card["location"]
        card["agency_look_ahead"] = agency_at_point(
            wind_hours["rows"],
            float(loc["lat"]),
            float(loc["lon"]),
            issued,
            issued + timedelta(hours=earth.weather_horizon_hours()),
        )
        card["input_note"] = (
            "pres_now is Open-Meteo sea-level pressure. "
            "gst_now is Open-Meteo 10 m wind speed in m/s, placed in the gust slot. "
            "The NDBC name is the function's own label."
        )

    measured_kp, predicted_kp, kp_counts = kp_inputs(kp["rows"], issued)
    solar_input = [{"kp": row["kp"], "t": row["t"].strftime("%Y-%m-%dT%H:%M:%SZ")} for row in measured_kp[-24:]]
    solar_cards = earth.solar_forecasts(solar_input, issued=issued)
    ahead_72 = [row for row in predicted_kp if row["t"] <= issued + timedelta(hours=72)]
    agency_peak = max(ahead_72, key=lambda row: row["kp"]) if ahead_72 else None

    events, quake_counts = quake_inputs(quakes["rows"])
    engine_events = [
        {"lat": ev["lat"], "lon": ev["lon"], "mag": ev["mag"], "time": ev["time"], "place": ev["place"]}
        for ev in events
    ]
    quake_cards = earth.earthquake_forecasts(engine_events, issued=issued)
    volcanic_cards = earth.volcanic_forecasts(engine_events, issued=issued)

    results = closed_results()
    rain = next((row for row in results if row.get("of") == "rain" and row.get("hour") == "2026-11-02T18"), None)
    if rain is None:
        raise SystemExit("closed rain hour 2026-11-02T18 is missing from the record")
    forecast = finite(rain.get("forecast"))
    later = finite(rain.get("later"))
    if forecast is None or later is None:
        raise SystemExit("closed rain hour is missing a number")

    stamp = issued.strftime("%Y%m%dT%H%M%SZ")
    SIDE.mkdir(parents=True, exist_ok=True)
    issuance = {
        "issued_at": issued.strftime("%Y-%m-%dT%H:%M:%SZ"),
        "pin_sha256": digest,
        "engine": str(LEAN_VENDOR / "fsot_earth_fluid_forecast.py"),
        "compute": str(LEAN_VENDOR / "fsot_compute.py"),
        "confirmed_held_out": 0,
        "closed_rain": {
            "hour": "2026-11-02T18",
            "of": "rain",
            "agency_forecast_mm": forecast,
            "agency_later_mm": later,
            "abs_difference_mm": abs(later - forecast),
            "src": rain.get("src"),
            "later_t": rain.get("later_t"),
            "record_t": rain.get("t"),
            "fsot_issued": False,
            "fsot_error_mm": None,
        },
        "weather": weather_cards,
        "solar": solar_cards,
        "earthquakes": quake_cards,
        "volcanic": volcanic_cards,
        "inputs": {
            "wind_grid_rows": len(wind_grid["rows"]),
            "usable_grid_points": len(buoys),
            "weather_skipped": weather_skip,
            "kp_measured_used": len(solar_input),
            "kp_counts": kp_counts,
            "quake_rows_used": len(events),
            "quake_skipped": quake_counts["skipped"],
        },
    }
    issuance_path = SIDE / f"issuance-{stamp}.json"
    issuance_path.write_text(json.dumps(issuance, indent=2), encoding="utf-8")

    times = [ev["t"] for ev in events]
    solar_card = round_card(solar_cards[0]) if solar_cards else None
    if solar_card is None:
        raise SystemExit("solar forecast returned no card")
    report = {
        "issued_at": issuance["issued_at"],
        "pin": digest,
        "closed_results": results,
        "closed_rain": {
            "hour": "2026-11-02T18",
            "forecast": forecast,
            "later": later,
            "abs_mm": abs(later - forecast),
        },
        "weather": {
            "grid_rows": len(wind_grid["rows"]),
            "usable": len(buoys),
            "storm_points": storm_points,
            "quiet_points": quiet_points,
            "gap_points": len(buoys) - storm_points - quiet_points,
            "cards": [],
        },
        "solar": {
            "measured_rows": len(measured_kp),
            "kp_now": measured_kp[-1]["kp"] if measured_kp else None,
            "kp_now_t": measured_kp[-1]["t"].strftime("%Y-%m-%dT%H:%M:%SZ") if measured_kp else None,
            "null_kp": kp_counts["null_kp"],
            "predicted_ahead_rows": len(predicted_kp),
            "predicted_behind": kp_counts["predicted_behind"],
            "agency_peak_kp": None if agency_peak is None else agency_peak["kp"],
            "agency_peak_t": None if agency_peak is None else agency_peak["t"].strftime("%Y-%m-%dT%H:%M:%SZ"),
            "card": solar_card,
        },
        "quakes": {
            "used": len(events),
            "skipped": quake_counts["skipped"],
            "first_t": min(times) if times else None,
            "last_t": max(times) if times else None,
            "horizon_days": earth.forecast_horizon_days(),
            "kernel_km": earth.kernel_km(),
            "cards": [round_card(card) for card in quake_cards],
        },
        "volcanic": [round_card(card) for card in volcanic_cards],
        "pending": pending_forecasts(),
        "pulled": {
            "wind-grid": wind_grid.get("pulled"),
            "wind-hours": wind_hours.get("pulled"),
            "kp": kp.get("pulled"),
            "quakes": quakes.get("pulled"),
        },
        "sources": {
            "wind-grid": wind_grid.get("source"),
            "wind-hours": wind_hours.get("source"),
            "kp": kp.get("source"),
            "quakes": quakes.get("source"),
        },
    }
    for card in weather_cards:
        brief = round_card(card)
        brief["agency_look_ahead"] = card["agency_look_ahead"]
        report["weather"]["cards"].append(brief)

    score_md = SIDE / "scorecard.md"
    score_json = SIDE / "scorecard.json"
    write_scorecard(score_md, report)
    score_json.write_text(json.dumps(report, indent=2), encoding="utf-8")

    print(f"issued {issuance['issued_at']}")
    print(f"held_out 0")
    print(f"closed_results {len(results)}")
    print(f"rain {forecast} later {later} abs {abs(later - forecast)} fsot_issued false")
    print(f"weather_cards {len(weather_cards)} usable {len(buoys)} storm {storm_points} quiet {quiet_points}")
    print(f"solar expect {solar_card['prediction'].get('expect_kp_ge_5')} kp_now {report['solar']['kp_now']} agency_peak {report['solar']['agency_peak_kp']}")
    print(f"quake_cards {len(quake_cards)} volcanic {len(volcanic_cards)} events {len(events)}")
    print(f"pending {len(report['pending'])}")
    print(f"wrote {issuance_path}")
    print(f"wrote {score_md}")


if __name__ == "__main__":
    main()
