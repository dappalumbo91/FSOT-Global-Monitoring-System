#!/usr/bin/env python3
"""Archive each monitor pull and issue a forward FSOT call from it.

Reads cache/series and cache/record. Writes only under FSOT/.
Does not edit the pinned engine and does not write the monitor cache.
"""

from __future__ import annotations

import json
import math
import sys
import time
import urllib.request
from datetime import datetime, timedelta, timezone
from pathlib import Path

import side_by_side as sbs

ROOT = sbs.ROOT
STAMPS = ROOT / "FSOT" / "stamps"
ARCHIVE = STAMPS / "archive"
SIDE = ROOT / "FSOT" / "side"
LEDGER = STAMPS / "ledger.jsonl"
OUTCOMES = STAMPS / "outcomes.jsonl"
MISSES = STAMPS / "misses.jsonl"
REPLAY = STAMPS / "replay-kp.json"
STATUS = STAMPS / "STATUS.md"
LOG = STAMPS / "loop.log"
ADOPTED = SIDE / "issuance-20261103T062124Z.json"
CONSUMED = ("wind-grid", "wind-hours", "kp", "quakes")
WEEK_URL = "https://earthquake.usgs.gov/earthquakes/feed/v1.0/summary/2.5_week.geojson"
WEEK_FILE = ARCHIVE / "usgs-2.5-week.geojson"
WEEK_MAX_AGE_S = 1800


def under_fsot(path: Path) -> Path:
    path.resolve().relative_to((ROOT / "FSOT").resolve())
    return path


def append_jsonl(path: Path, row: dict) -> None:
    under_fsot(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(row, separators=(",", ":")) + "\n")


def read_jsonl(path: Path) -> list[dict]:
    if not path.exists():
        return []
    rows = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line.strip():
            continue
        rows.append(json.loads(line))
    return rows


def log_line(text: str) -> None:
    under_fsot(LOG)
    LOG.parent.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    with LOG.open("a", encoding="utf-8") as handle:
        handle.write(f"{stamp} {text}\n")


def parse_pulled(text: object) -> datetime | None:
    if not isinstance(text, str) or not text.strip():
        return None
    cleaned = " ".join(text.split()).removesuffix(" UTC")
    try:
        return datetime.strptime(cleaned, "%Y-%m-%d %H:%M:%S").replace(tzinfo=timezone.utc)
    except ValueError:
        return sbs.as_utc(text)


def pull_key(series: dict[str, dict]) -> str:
    return "|".join(f"{name}:{series[name].get('pulled')}" for name in ("wind-grid", "kp", "quakes"))


def card_parts(card: dict) -> tuple:
    loc = card.get("location") or {}
    pred = card.get("predicted") or {}
    lat = sbs.finite(loc.get("lat"))
    lon = sbs.finite(loc.get("lon"))
    return (
        card.get("kind"),
        pred.get("class"),
        pred.get("valve_state"),
        pred.get("expect_kp_ge_5"),
        pred.get("expect_event"),
        pred.get("mag_min"),
        pred.get("pres_max_hpa"),
        pred.get("gst_min_ms"),
        loc.get("basin"),
        None if lat is None else round(lat, 1),
        None if lon is None else round(lon, 1),
    )


def signature(cards: list[dict]) -> str:
    return json.dumps([card_parts(card) for card in cards], separators=(",", ":"))


def load_cards(path: Path) -> list[dict]:
    data = sbs.read_json(path)
    cards = []
    for key in ("weather", "solar", "earthquakes", "volcanic"):
        rows = data.get(key) or []
        if isinstance(rows, list):
            cards.extend(rows)
    return cards


def known_ids(path: Path, field: str) -> set[str]:
    return {str(row.get(field)) for row in read_jsonl(path) if row.get(field) is not None}


def archive_kp(rows: list[dict]) -> int:
    path = ARCHIVE / "kp.jsonl"
    seen = known_ids(path, "key")
    added = 0
    for row in rows:
        value = sbs.finite(row.get("kp"))
        stamp = row.get("t")
        status = str(row.get("observed") or "")
        if value is None or not stamp:
            continue
        key = f"{stamp}|{status}|{value}"
        if key in seen:
            continue
        seen.add(key)
        append_jsonl(path, {"key": key, "t": stamp, "observed": status, "kp": value})
        added += 1
    return added


def archive_quakes(rows: list[dict]) -> int:
    path = ARCHIVE / "quakes.jsonl"
    seen = known_ids(path, "id")
    added = 0
    for row in rows:
        event_id = row.get("id")
        if not event_id or str(event_id) in seen:
            continue
        lat = sbs.finite(row.get("lat"))
        lon = sbs.finite(row.get("lon"))
        mag = sbs.finite(row.get("mag"))
        if lat is None or lon is None or mag is None:
            continue
        seen.add(str(event_id))
        append_jsonl(
            path,
            {
                "id": event_id,
                "t": row.get("t"),
                "lat": lat,
                "lon": lon,
                "mag": mag,
                "place": row.get("place") or "",
            },
        )
        added += 1
    return added


def archive_grid(pulled: object, rows: list[dict]) -> bool:
    when = parse_pulled(pulled)
    if when is None:
        return False
    name = when.strftime("%Y%m%dT%H%M%SZ") + ".json"
    path = ARCHIVE / "grids" / name
    if path.exists():
        return False
    compact = []
    for row in rows:
        lat = sbs.finite(row.get("lat"))
        lon = sbs.finite(row.get("lon"))
        pres = sbs.finite(row.get("pressure_hpa"))
        wind = sbs.finite(row.get("wind_ms"))
        if lat is None or lon is None or pres is None or wind is None:
            continue
        compact.append(
            {"t": row.get("t"), "lat": lat, "lon": lon, "pressure_hpa": pres, "wind_ms": wind}
        )
    under_fsot(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps({"pulled_at": when.strftime("%Y-%m-%dT%H:%M:%SZ"), "rows": compact}),
        encoding="utf-8",
    )
    return True


def archived_kp() -> list[dict]:
    rows = []
    for row in read_jsonl(ARCHIVE / "kp.jsonl"):
        stamp = sbs.as_utc(row.get("t"))
        value = sbs.finite(row.get("kp"))
        if stamp is None or value is None:
            continue
        rows.append({"t": stamp, "kp": value, "observed": str(row.get("observed") or "").lower()})
    rows.sort(key=lambda item: item["t"])
    return rows


def archived_quakes() -> list[dict]:
    rows = []
    for row in read_jsonl(ARCHIVE / "quakes.jsonl"):
        stamp = sbs.as_utc(row.get("t"))
        lat = sbs.finite(row.get("lat"))
        lon = sbs.finite(row.get("lon"))
        mag = sbs.finite(row.get("mag"))
        if stamp is None or lat is None or lon is None or mag is None:
            continue
        rows.append({"t": stamp, "lat": lat, "lon": lon, "mag": mag, "id": row.get("id")})
    return rows


def grid_snapshots() -> list[dict]:
    folder = ARCHIVE / "grids"
    if not folder.exists():
        return []
    snaps = []
    for path in sorted(folder.glob("*.json")):
        data = sbs.read_json(path)
        when = sbs.as_utc(data.get("pulled_at"))
        if when is None:
            continue
        snaps.append({"pulled_at": when, "rows": data.get("rows") or []})
    return snaps


def refresh_week_catalog(now: datetime) -> dict:
    """Keep a USGS week file under FSOT/stamps/archive. Leave the monitor cache alone."""
    age = None
    if WEEK_FILE.exists() and WEEK_FILE.stat().st_size > 1000:
        age = int(now.timestamp() - WEEK_FILE.stat().st_mtime)
        if age < WEEK_MAX_AGE_S:
            return {"ok": True, "file": WEEK_FILE.name, "age_s": age, "fetched": False}
    try:
        request = urllib.request.Request(WEEK_URL, headers={"User-Agent": "FSOT-stamp-loop/0.1"})
        with urllib.request.urlopen(request, timeout=20) as response:
            body = response.read()
        if len(body) < 1000:
            raise RuntimeError(f"week catalog short {len(body)}")
        under_fsot(WEEK_FILE)
        WEEK_FILE.parent.mkdir(parents=True, exist_ok=True)
        WEEK_FILE.write_bytes(body)
        return {"ok": True, "file": WEEK_FILE.name, "age_s": 0, "fetched": True, "bytes": len(body)}
    except Exception as exc:
        if WEEK_FILE.exists() and WEEK_FILE.stat().st_size > 1000:
            return {
                "ok": True,
                "file": WEEK_FILE.name,
                "age_s": age if age is not None else None,
                "fetched": False,
                "stale_error": str(exc),
            }
        return {"ok": False, "error": str(exc)}


def events_from_week(doc: dict, issued: datetime) -> dict[str, dict]:
    issued_ms = int(issued.timestamp() * 1000)
    found = {}
    for feature in doc.get("features") or []:
        props = feature.get("properties") or {}
        coords = (feature.get("geometry") or {}).get("coordinates") or []
        event_id = feature.get("id") or props.get("code")
        mag = sbs.finite(props.get("mag"))
        when = props.get("time")
        if event_id is None or mag is None or when is None or len(coords) < 2:
            continue
        when_ms = int(when)
        if when_ms > issued_ms:
            continue
        lat = sbs.finite(coords[1])
        lon = sbs.finite(coords[0])
        if lat is None or lon is None:
            continue
        found[str(event_id)] = {
            "id": str(event_id),
            "lat": lat,
            "lon": lon,
            "mag": mag,
            "time": when_ms,
            "place": str(props.get("place") or ""),
            "type": str(props.get("type") or ""),
        }
    return found


def events_from_day(rows: list[dict], issued: datetime) -> dict[str, dict]:
    issued_ms = int(issued.timestamp() * 1000)
    found = {}
    for row in rows:
        event_id = row.get("id")
        stamp = sbs.as_utc(row.get("t"))
        lat = sbs.finite(row.get("lat"))
        lon = sbs.finite(row.get("lon"))
        mag = sbs.finite(row.get("mag"))
        if not event_id or stamp is None or lat is None or lon is None or mag is None:
            continue
        when_ms = int(stamp.timestamp() * 1000)
        if when_ms > issued_ms:
            continue
        found[str(event_id)] = {
            "id": str(event_id),
            "lat": lat,
            "lon": lon,
            "mag": mag,
            "time": when_ms,
            "place": str(row.get("place") or ""),
            "type": str(row.get("type") or ""),
        }
    return found


def quake_catalog(day_rows: list[dict], issued: datetime) -> tuple[list[dict], dict]:
    meta = refresh_week_catalog(issued)
    week: dict[str, dict] = {}
    if meta.get("ok"):
        try:
            week = events_from_week(json.loads(WEEK_FILE.read_text(encoding="utf-8")), issued)
        except Exception as exc:
            meta = {"ok": False, "error": str(exc)}
            week = {}
    day = events_from_day(day_rows, issued)
    merged = dict(week)
    added = 0
    for event_id, event in day.items():
        if event_id not in merged:
            merged[event_id] = event
            added += 1
    note = {
        "source": (
            "USGS 2.5_week plus day-feed ids absent from that file"
            if week
            else "USGS day feed"
        ),
        "week_events": len(week),
        "day_events": len(day),
        "day_ids_added": added,
        "used": len(merged),
        "week_file": meta.get("file"),
        "week_age_s": meta.get("age_s"),
        "week_fetched": bool(meta.get("fetched")),
        "week_error": meta.get("error") or meta.get("stale_error"),
    }
    engine = [
        {
            "lat": event["lat"],
            "lon": event["lon"],
            "mag": event["mag"],
            "time": event["time"],
            "place": event["place"],
            "type": event["type"],
        }
        for event in merged.values()
    ]
    return engine, note


def issue_cards(earth, series: dict[str, dict], issued: datetime) -> list[dict]:
    buoys, _skipped = sbs.weather_inputs(series["wind-grid"]["rows"])
    weather = earth.weather_forecasts(buoys, issued=issued)
    for card in weather:
        loc = card["location"]
        card["agency_look_ahead"] = sbs.agency_at_point(
            series["wind-hours"]["rows"],
            float(loc["lat"]),
            float(loc["lon"]),
            issued,
            issued + timedelta(hours=earth.weather_horizon_hours()),
        )
        card["input_note"] = (
            "pres_now is Open-Meteo sea-level pressure. "
            "gst_now is Open-Meteo 10 m wind speed in m/s. "
            "The NDBC name is the function label."
        )
    measured, _ahead, _counts = sbs.kp_inputs(series["kp"]["rows"], issued)
    solar_input = [{"kp": row["kp"]} for row in measured[-24:]]
    solar = earth.solar_forecasts(solar_input, issued=issued)
    engine_events, catalog = quake_catalog(series["quakes"]["rows"], issued)
    quakes = earth.earthquake_forecasts(engine_events, issued=issued)
    volcanic = earth.volcanic_forecasts(engine_events, issued=issued)
    for card in quakes + volcanic:
        card["catalog"] = catalog
    return weather + solar + quakes + volcanic


def write_issuance(earth, series: dict[str, dict], issued: datetime, cards: list[dict]) -> Path:
    stamp = issued.strftime("%Y%m%dT%H%M%SZ")
    path = SIDE / f"issuance-{stamp}.json"
    under_fsot(path)
    weather = [card for card in cards if card.get("kind") == "weather"]
    solar = [card for card in cards if card.get("kind") == "solar"]
    quakes = [card for card in cards if card.get("kind") == "earthquake"]
    volcanic = [card for card in cards if card.get("kind") == "volcanic"]
    body = {
        "issued_at": issued.strftime("%Y-%m-%dT%H:%M:%SZ"),
        "pin_sha256": sbs.PIN,
        "engine": str(sbs.LEAN_VENDOR / "fsot_earth_fluid_forecast.py"),
        "confirmed_held_out": 0,
        "counts_when": "scored only against a measurement whose time is after issued_at",
        "quake_catalog": (quakes[0].get("catalog") if quakes else None),
        "weather": weather,
        "solar": solar,
        "earthquakes": quakes,
        "volcanic": volcanic,
        "horizon": {
            "weather_hours": earth.weather_horizon_hours(),
            "earthquake_days": earth.forecast_horizon_days(),
            "kernel_km": earth.kernel_km(),
            "solar_hours": 72,
        },
    }
    path.write_text(json.dumps(body, indent=2), encoding="utf-8")
    return path


def adopt_first(cards_now_sig: str) -> str | None:
    if not ADOPTED.exists():
        return None
    if any(row.get("action") == "adopted" for row in read_jsonl(LEDGER)):
        return signature(load_cards(ADOPTED))
    adopted_cards = load_cards(ADOPTED)
    adopted_sig = signature(adopted_cards)
    append_jsonl(
        LEDGER,
        {
            "stamp": "2026-11-03T06:21:24Z",
            "action": "adopted",
            "counted": True,
            "file": ADOPTED.name,
            "cards": len(adopted_cards),
            "same_as_latest_pull": adopted_sig == cards_now_sig,
        },
    )
    return adopted_sig


def last_pull_key() -> str | None:
    rows = read_jsonl(LEDGER)
    for row in reversed(rows):
        if row.get("pull_key"):
            return str(row["pull_key"])
    return None


def last_counted_signature() -> str | None:
    rows = read_jsonl(LEDGER)
    for row in reversed(rows):
        if row.get("counted") and row.get("signature"):
            return str(row["signature"])
        if row.get("action") == "adopted":
            return signature(load_cards(ADOPTED))
    return None


def card_label(card: dict) -> str:
    loc = card.get("location") or {}
    pred = card.get("predicted") or {}
    lat = sbs.finite(loc.get("lat"))
    lon = sbs.finite(loc.get("lon"))
    place = loc.get("name") or ""
    lat_s = "" if lat is None else f"{lat:.0f}"
    lon_s = "" if lon is None else f"{lon:.0f}"
    return f"new card {card.get('kind')} {pred.get('class')} {place} {lat_s},{lon_s}".strip()


def counted_cards() -> list[dict]:
    cards = []
    for entry in read_jsonl(LEDGER):
        if not entry.get("counted") or not entry.get("file"):
            continue
        path = SIDE / str(entry["file"])
        if path.exists():
            cards.extend(load_cards(path))
    return cards


def live_parts(now: datetime) -> set[tuple]:
    found = set()
    for card in counted_cards():
        valid = sbs.as_utc(card.get("valid_to"))
        if valid is not None and valid > now:
            found.add(card_parts(card))
    return found


def weather_slot(card: dict) -> tuple | None:
    if card.get("kind") != "weather":
        return None
    loc = card.get("location") or {}
    pred = card.get("predicted") or {}
    basin = loc.get("basin")
    kind = pred.get("class")
    if not basin or not kind:
        return None
    return (kind, basin)


def live_weather_slots(now: datetime) -> set[tuple]:
    found = set()
    for card in counted_cards():
        valid = sbs.as_utc(card.get("valid_to"))
        slot = weather_slot(card)
        if slot is not None and valid is not None and valid > now:
            found.add(slot)
    return found


def haversine_km(lat1: float, lon1: float, lat2: float, lon2: float) -> float:
    p1 = math.radians(lat1)
    p2 = math.radians(lat2)
    dp = math.radians(lat2 - lat1)
    dl = math.radians(lon2 - lon1)
    a = math.sin(dp / 2.0) ** 2 + math.cos(p1) * math.cos(p2) * math.sin(dl / 2.0) ** 2
    return 6371.0 * 2.0 * math.asin(min(1.0, math.sqrt(a)))


def quake_anchor(card: dict) -> tuple | None:
    if card.get("kind") != "earthquake":
        return None
    loc = card.get("location") or {}
    pred = card.get("predicted") or {}
    lat = sbs.finite(loc.get("lat"))
    lon = sbs.finite(loc.get("lon"))
    radius = sbs.finite(loc.get("radius_km"))
    if lat is None or lon is None or radius is None:
        return None
    return (lat, lon, radius, pred.get("class"), pred.get("mag_min"), pred.get("expect_event"))


def same_quake_cell(card: dict, other: dict) -> bool:
    # The first open card keeps the cell, including when a later catalog would relabel the valve.
    left = quake_anchor(card)
    right = quake_anchor(other)
    if left is None or right is None:
        return False
    return haversine_km(left[0], left[1], right[0], right[1]) <= right[2]


def inside_open_quake(card: dict, now: datetime) -> bool:
    for other in counted_cards():
        valid = sbs.as_utc(other.get("valid_to"))
        if valid is None or valid <= now:
            continue
        if same_quake_cell(card, other):
            return True
    return False


def earlier_same_quake(card: dict, prior: list[dict]) -> str | None:
    issued = sbs.as_utc(card.get("issued_at"))
    if issued is None or quake_anchor(card) is None:
        return None
    for other in prior:
        if not same_quake_cell(card, other):
            continue
        other_issued = sbs.as_utc(other.get("issued_at"))
        other_valid = sbs.as_utc(other.get("valid_to"))
        if other_issued is None or other_valid is None:
            continue
        if other_issued < issued and other_valid > issued:
            return str(other.get("id") or "")
    return None


def earlier_same_call(card: dict, prior: list[dict]) -> str | None:
    issued = sbs.as_utc(card.get("issued_at"))
    parts = card_parts(card)
    if issued is None:
        return None
    for other in prior:
        if str(other.get("id") or "") == str(card.get("id") or ""):
            continue
        if card_parts(other) != parts:
            continue
        other_issued = sbs.as_utc(other.get("issued_at"))
        other_valid = sbs.as_utc(other.get("valid_to"))
        if other_issued is None or other_valid is None:
            continue
        if other_issued < issued and other_valid > issued:
            return str(other.get("id") or "")
    return None


def earlier_same_slot(card: dict, prior: list[dict]) -> str | None:
    slot = weather_slot(card)
    issued = sbs.as_utc(card.get("issued_at"))
    if slot is None or issued is None:
        return None
    for other in prior:
        if weather_slot(other) != slot:
            continue
        other_issued = sbs.as_utc(other.get("issued_at"))
        other_valid = sbs.as_utc(other.get("valid_to"))
        if other_issued is None or other_valid is None:
            continue
        if other_issued < issued and other_valid > issued:
            return str(other.get("id") or "")
    return None


def kp_reached(samples: list[dict], start: datetime, end: datetime) -> tuple[bool, int]:
    window = [
        row
        for row in samples
        if row["observed"] in {"observed", "estimated"} and start < row["t"] <= end
    ]
    return any(row["kp"] >= 5.0 for row in window), len(window)


def score_solar(card: dict, samples: list[dict], now: datetime) -> dict | None:
    issued = sbs.as_utc(card.get("issued_at"))
    valid = sbs.as_utc(card.get("valid_to"))
    expect = (card.get("predicted") or {}).get("expect_kp_ge_5")
    if issued is None or valid is None or not isinstance(expect, bool):
        return None
    reached, count = kp_reached(samples, issued, valid)
    if reached:
        hit = expect
        return {"result": "hit" if hit else "miss", "kp_ge_5": True, "samples": count}
    if now < valid:
        return None
    last = max((row["t"] for row in samples if row["observed"] in {"observed", "estimated"}), default=None)
    if last is None or last < valid - timedelta(hours=3):
        return {"result": "unmeasured", "kp_ge_5": False, "samples": count}
    return {"result": "hit" if not expect else "miss", "kp_ge_5": False, "samples": count}


def point_samples(card: dict, snaps: list[dict], start: datetime, end: datetime) -> list[dict]:
    loc = card.get("location") or {}
    lat = sbs.finite(loc.get("lat"))
    lon = sbs.finite(loc.get("lon"))
    if lat is None or lon is None:
        return []
    found = []
    for snap in snaps:
        if not (start < snap["pulled_at"] <= end):
            continue
        for row in snap["rows"]:
            rlat = sbs.finite(row.get("lat"))
            rlon = sbs.finite(row.get("lon"))
            pres = sbs.finite(row.get("pressure_hpa"))
            wind = sbs.finite(row.get("wind_ms"))
            if rlat is None or rlon is None or pres is None or wind is None:
                continue
            if abs(rlat - lat) < 0.05 and abs(rlon - lon) < 0.05:
                found.append({"pres": pres, "wind": wind, "t": snap["pulled_at"]})
    return found


def score_weather(card: dict, snaps: list[dict], now: datetime) -> dict | None:
    issued = sbs.as_utc(card.get("issued_at"))
    valid = sbs.as_utc(card.get("valid_to"))
    kind = (card.get("predicted") or {}).get("class")
    if issued is None or valid is None or kind not in {"storm_sector", "quiet_sector"}:
        return None
    if now < valid:
        return None
    samples = point_samples(card, snaps, issued, valid)
    if not samples:
        return {"result": "unmeasured", "samples": 0}
    if kind == "storm_sector":
        stayed_quiet = all(sample["pres"] >= 1010.0 and sample["wind"] < 8.0 for sample in samples)
        return {"result": "miss" if stayed_quiet else "hit", "samples": len(samples)}
    failed = any(sample["pres"] < 1005.0 or sample["wind"] >= 12.0 for sample in samples)
    return {"result": "miss" if failed else "hit", "samples": len(samples)}


def score_quake(card: dict, events: list[dict], now: datetime, kernel) -> dict | None:
    issued = sbs.as_utc(card.get("issued_at"))
    valid = sbs.as_utc(card.get("valid_to"))
    pred = card.get("predicted") or {}
    expect = pred.get("expect_event")
    mag_min = sbs.finite(pred.get("mag_min"))
    loc = card.get("location") or {}
    lat = sbs.finite(loc.get("lat"))
    lon = sbs.finite(loc.get("lon"))
    radius = sbs.finite(loc.get("radius_km")) or kernel
    if issued is None or valid is None or lat is None or lon is None or mag_min is None or not isinstance(expect, bool):
        return None
    hits = []
    for event in events:
        if not (issued < event["t"] <= valid) or event["mag"] < mag_min:
            continue
        if kernel(lat, lon, event["lat"], event["lon"]) <= radius:
            hits.append(event["id"])
    if hits:
        return {"result": "hit" if expect else "miss", "events": hits}
    if now < valid:
        return None
    return {"result": "miss" if expect else "hit", "events": []}


def score_open(earth, now: datetime) -> list[dict]:
    done = {str(row.get("card_id")) for row in read_jsonl(OUTCOMES)}
    kp_rows = archived_kp()
    quakes = archived_quakes()
    snaps = grid_snapshots()
    fresh = []
    seen: list[dict] = []
    for entry in read_jsonl(LEDGER):
        if not entry.get("counted") or not entry.get("file"):
            continue
        path = SIDE / str(entry["file"])
        if not path.exists():
            continue
        for card in load_cards(path):
            card_id = str(card.get("id") or "")
            if not card_id or card_id in done:
                seen.append(card)
                continue
            same = earlier_same_slot(card, seen) or earlier_same_quake(card, seen) or earlier_same_call(card, seen)
            seen.append(card)
            kind = card.get("kind")
            if same:
                outcome = {"result": "duplicate", "same_as": same}
            else:
                if kind == "solar":
                    outcome = score_solar(card, kp_rows, now)
                elif kind == "weather":
                    outcome = score_weather(card, snaps, now)
                elif kind == "earthquake":
                    outcome = score_quake(card, quakes, now, earth.haversine_km)
                else:
                    outcome = None
            if outcome is None:
                continue
            row = {
                "scored_at": now.strftime("%Y-%m-%dT%H:%M:%SZ"),
                "file": path.name,
                "card_id": card_id,
                "kind": kind,
                **outcome,
            }
            append_jsonl(OUTCOMES, row)
            if outcome["result"] == "miss":
                append_jsonl(MISSES, row)
            done.add(card_id)
            fresh.append(row)
    return fresh


def replay_kp(measured: list[dict]) -> dict:
    sys.path.insert(0, str(sbs.LEAN_VENDOR))
    import fsot_earth_fluid_forecast as earth

    if not measured:
        return {"windows": 0, "resolved": 0, "hits": 0, "misses": 0, "open": 0}
    last = measured[-1]["t"]
    windows = []
    for row in measured:
        start = row["t"]
        hist = [item for item in measured if item["t"] <= start][-24:]
        cards = earth.solar_forecasts([{"kp": item["kp"]} for item in hist], issued=start)
        if not cards:
            continue
        expect = bool(cards[0]["predicted"]["expect_kp_ge_5"])
        end = start + timedelta(hours=72)
        future = [item for item in measured if start < item["t"] <= end]
        reached = any(item["kp"] >= 5.0 for item in future)
        closed = end <= last
        resolved = reached or closed
        if not resolved:
            result = "open"
        elif expect == reached or (not expect and closed and not reached):
            result = "hit" if (expect and reached) or (not expect and not reached) else "miss"
        else:
            result = "miss"
        if expect and not reached and not closed:
            result = "open"
            resolved = False
        windows.append(
            {
                "t": start.strftime("%Y-%m-%dT%H:%M:%SZ"),
                "expect_kp_ge_5": expect,
                "reached": reached,
                "closed": closed,
                "result": result,
                "future_rows": len(future),
                "kp_at_stamp": row["kp"],
            }
        )
    resolved_rows = [row for row in windows if row["result"] in {"hit", "miss"}]
    picked = []
    next_ok = None
    for row in resolved_rows:
        start = sbs.as_utc(row["t"])
        if start is None:
            continue
        if next_ok is not None and start < next_ok:
            continue
        picked.append(row)
        next_ok = start + timedelta(hours=72)
    max_kp = max(row["kp"] for row in measured)
    report = {
        "role": "frozen-formula replay computed after the hours; not held-out",
        "measured_rows": len(measured),
        "first_t": measured[0]["t"].strftime("%Y-%m-%dT%H:%M:%SZ"),
        "last_t": last.strftime("%Y-%m-%dT%H:%M:%SZ"),
        "max_kp": max_kp,
        "overlapping": {
            "resolved": len(resolved_rows),
            "hits": sum(1 for row in resolved_rows if row["result"] == "hit"),
            "misses": sum(1 for row in resolved_rows if row["result"] == "miss"),
            "open": sum(1 for row in windows if row["result"] == "open"),
            "storm_calls": sum(1 for row in resolved_rows if row["expect_kp_ge_5"]),
            "quiet_calls": sum(1 for row in resolved_rows if not row["expect_kp_ge_5"]),
        },
        "nonoverlapping_72h": {
            "resolved": len(picked),
            "hits": sum(1 for row in picked if row["result"] == "hit"),
            "misses": sum(1 for row in picked if row["result"] == "miss"),
            "stamps": [row["t"] for row in picked],
        },
        "windows": windows,
    }
    under_fsot(REPLAY)
    REPLAY.write_text(json.dumps(report, indent=2), encoding="utf-8")
    return report


def pending_cards(now: datetime) -> list[tuple[datetime, dict]]:
    done = {
        row.get("card_id")
        for row in read_jsonl(OUTCOMES)
        if row.get("result") in {"hit", "miss", "unmeasured", "duplicate"}
    }
    pending = []
    for card in counted_cards():
        if card.get("id") in done:
            continue
        valid = sbs.as_utc(card.get("valid_to"))
        if valid is not None and valid > now:
            pending.append((valid, card))
    pending.sort(key=lambda item: item[0])
    return pending


def write_status(now: datetime, series: dict[str, dict], action: str, file_name: str, fresh: list[dict], replay: dict) -> None:
    held = sorted(path.stem for path in sbs.SERIES.glob("*.json"))
    ledger = read_jsonl(LEDGER)
    counted = [row for row in ledger if row.get("counted")]
    carried = [row for row in ledger if row.get("action") == "carry"]
    outcomes = read_jsonl(OUTCOMES)
    scored = [row for row in outcomes if row.get("result") in {"hit", "miss", "unmeasured"}]
    duplicates = [row for row in outcomes if row.get("result") == "duplicate"]
    overlap = replay.get("overlapping") or {}
    apart = replay.get("nonoverlapping_72h") or {}
    lines = [
        "# FSOT stamp board",
        "",
        f"Updated {now.strftime('%Y-%m-%dT%H:%M:%SZ')}.",
        f"Scored calls written before the measurement: {len(scored)}.",
        "This board is the diary. The monitor cache was not written.",
        "",
        "## Goal",
        "",
        "Usable forward calls from the pinned FSOT engine.",
        "Open this file for the score. The loop appends a hit or a miss when a window closes.",
        "Cards already written stay as written. A new quake card uses the USGS 7-day catalog.",
        "",
        "## This pull",
        "",
        f"Action `{action}` file `{file_name}`.",
        f"wind-grid pulled `{series['wind-grid'].get('pulled')}`.",
        f"kp pulled `{series['kp'].get('pulled')}`.",
        f"quakes pulled `{series['quakes'].get('pulled')}`.",
        "",
        "## Counted calls",
        "",
        f"Counted issuances: {len(counted)}. Carried pulls: {len(carried)}.",
        f"Hits {sum(1 for row in scored if row.get('result') == 'hit')}. Misses {sum(1 for row in scored if row.get('result') == 'miss')}. Unmeasured {sum(1 for row in scored if row.get('result') == 'unmeasured')}.",
        f"Duplicate cards left out of the score: {len(duplicates)}. Misses logged: {len(read_jsonl(MISSES))}.",
        "A carried pull repeats an open card and does not add a test.",
        "A later card with the same class and place as a card still open is a duplicate.",
        "A hit or a miss is a call this loop wrote before the measurement.",
        "",
        "## Kp replay diagnostic",
        "",
        "The frozen Kp rule was walked across the measured rows already in the cache.",
        "Those calls were computed after the hours, so this replay is not held-out.",
        f"Measured Kp rows {replay.get('measured_rows')} from `{replay.get('first_t')}` through `{replay.get('last_t')}`. Maximum Kp {replay.get('max_kp')}.",
        f"Overlapping 3-hour stamps: {overlap.get('hits', 0)} hits, {overlap.get('misses', 0)} misses, {overlap.get('open', 0)} still open, of {overlap.get('resolved', 0)} resolved.",
        f"Among the resolved overlapping stamps, quiet calls {overlap.get('quiet_calls', 0)} and storm calls {overlap.get('storm_calls', 0)}.",
        f"Non-overlapping 72-hour steps: {apart.get('hits', 0)} hits and {apart.get('misses', 0)} misses across {apart.get('resolved', 0)} windows.",
        "",
        "## Series on the clock",
        "",
        "The pinned forecast functions take the wind grid, the Kp table, and USGS earthquakes.",
        "A new quake card is built from the 7-day catalog, plus day-feed ids that file does not yet have.",
        "The day feed still scores the card. Wind-hours are the agency look-ahead stored beside a weather card.",
        "The other series stay archived by the monitor and have no forecast function on this pin.",
        "",
        ", ".join(name for name in held if name not in CONSUMED) or "none",
        "",
        "## Still open",
        "",
    ]
    pending = pending_cards(now)
    if pending:
        for valid, card in pending:
            loc = card.get("location") or {}
            pred = card.get("predicted") or {}
            lines.append(
                f"- `{card.get('id')}` {card.get('kind')} through {valid.strftime('%Y-%m-%dT%H:%M:%SZ')} "
                f"{loc.get('name') or ''} valve `{pred.get('valve_state')}`."
            )
    else:
        lines.append("No card is waiting on a later measurement.")
    lines.extend(
        [
            "",
            "## Open scoring",
            "",
        ]
    )
    if fresh:
        for row in fresh:
            lines.append(f"- `{row['card_id']}` {row['kind']} {row['result']}.")
    else:
        lines.append("No open card resolved on this pass.")
    lines.append("")
    under_fsot(STATUS)
    STATUS.write_text("\n".join(lines), encoding="utf-8")


def run_once(report: bool) -> str | None:
    sbs.pin_ok()
    sys.path.insert(0, str(sbs.LEAN_VENDOR))
    import fsot_earth_fluid_forecast as earth

    now = datetime.now(timezone.utc).replace(microsecond=0)
    names = ("wind-grid", "wind-hours", "kp", "quakes")
    series = {name: sbs.load_series(name) for name in names}
    archive_kp(series["kp"]["rows"])
    archive_quakes(series["quakes"]["rows"])
    archive_grid(series["wind-grid"].get("pulled"), series["wind-grid"]["rows"])
    measured = [
        row
        for row in archived_kp()
        if row["observed"] in {"observed", "estimated"} and row["t"] <= now
    ]
    replay = replay_kp(measured)
    key = pull_key(series)
    cards = issue_cards(earth, series, now)
    current_sig = signature(cards)
    adopt_first(current_sig)
    action_line = None
    if key == last_pull_key():
        fresh = score_open(earth, now)
        write_status(now, series, "idle", "", fresh, replay)
        log_line(f"idle pull {key} resolved {len(fresh)}")
        real = [row for row in fresh if row.get("result") in {"hit", "miss", "unmeasured"}]
        if real:
            action_line = "ACTION_REQUIRED: " + "; ".join(
                f"{row['card_id']} {row['result']}" for row in real
            )
        return status_text(replay, fresh) if report else action_line

    open_parts = live_parts(now)
    open_slots = live_weather_slots(now)
    novel = [
        card
        for card in cards
        if card_parts(card) not in open_parts
        and weather_slot(card) not in open_slots
        and not inside_open_quake(card, now)
    ]
    if not novel:
        action = "carry"
        file_name = ""
        for row in reversed(read_jsonl(LEDGER)):
            if row.get("counted") and row.get("file"):
                file_name = str(row["file"])
                break
        append_jsonl(
            LEDGER,
            {
                "stamp": now.strftime("%Y-%m-%dT%H:%M:%SZ"),
                "action": "carry",
                "counted": False,
                "file": file_name,
                "pull_key": key,
                "signature": current_sig,
            },
        )
    else:
        action = "issue"
        path = write_issuance(earth, series, now, novel)
        file_name = path.name
        append_jsonl(
            LEDGER,
            {
                "stamp": now.strftime("%Y-%m-%dT%H:%M:%SZ"),
                "action": "issue",
                "counted": True,
                "file": file_name,
                "pull_key": key,
                "signature": current_sig,
                "cards": len(novel),
                "novel": [card_label(card) for card in novel],
            },
        )
        action_line = "ACTION_REQUIRED: " + "; ".join(card_label(card) for card in novel)
    fresh = score_open(earth, now)
    write_status(now, series, action, file_name, fresh, replay)
    log_line(f"{action} {file_name} pull {key} novel {len(novel)} resolved {len(fresh)}")
    real = [row for row in fresh if row.get("result") in {"hit", "miss", "unmeasured"}]
    if real:
        scored_line = "ACTION_REQUIRED: " + "; ".join(
            f"{row['card_id']} {row['result']}" for row in real
        )
        action_line = scored_line if action_line is None else f"{action_line}; {scored_line}"
    if report:
        return status_text(replay, fresh, action, file_name)
    return action_line


def status_text(replay: dict, fresh: list[dict], action: str = "", file_name: str = "") -> str:
    overlap = replay.get("overlapping") or {}
    apart = replay.get("nonoverlapping_72h") or {}
    lines = [
        f"action {action} file {file_name}".strip(),
        f"held_out 0",
        f"replay_overlap hits {overlap.get('hits')} misses {overlap.get('misses')} open {overlap.get('open')} max_kp {replay.get('max_kp')}",
        f"replay_72h hits {apart.get('hits')} misses {apart.get('misses')} n {apart.get('resolved')}",
        f"newly_resolved {len(fresh)}",
    ]
    return "\n".join(lines)


def watch() -> None:
    while True:
        try:
            message = run_once(report=False)
        except Exception as exc:
            log_line(f"FAILED {exc}")
            print(f"FAILED: {exc}", flush=True)
            time.sleep(30)
            continue
        if message:
            print(message, flush=True)
        time.sleep(300)


if __name__ == "__main__":
    if "--watch" in sys.argv:
        watch()
    else:
        print(run_once(report=True))
