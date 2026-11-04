#!/usr/bin/env python3
"""Copy monitor series into numeric CSV for Octave. Does not write the cache."""

from __future__ import annotations

import json
import sys
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SERIES = ROOT / "cache" / "series"
OUT = ROOT / "FSOT" / "octave" / "data"
USGS = "https://earthquake.usgs.gov/earthquakes/feed/v1.0/summary/2.5_week.geojson"


def as_unix(text: object) -> float | None:
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
    return parsed.timestamp()


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


def write_rows(name: str, header: str, rows: list[list[float]]) -> None:
    path = OUT / name
    lines = [header] + [",".join(f"{value:.6f}" for value in row) for row in rows]
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"{name} {len(rows)}")


def series_rows(name: str) -> list[dict]:
    data = json.loads((SERIES / f"{name}.json").read_text(encoding="utf-8"))
    rows = data.get("rows")
    if not isinstance(rows, list):
        raise RuntimeError(f"{name} has no rows")
    print(f"source {name} pulled {data.get('pulled')} n {len(rows)}")
    return [row for row in rows if isinstance(row, dict)]


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    kp_rows = []
    for row in series_rows("kp"):
        stamp = as_unix(row.get("t"))
        value = finite(row.get("kp"))
        status = str(row.get("observed") or "").strip().lower()
        if stamp is None or value is None or status not in {"observed", "estimated"}:
            continue
        kp_rows.append([stamp, value])
    kp_rows.sort()
    write_rows("kp.csv", "unix,kp", kp_rows)

    dst_rows = []
    for row in series_rows("dst"):
        stamp = as_unix(row.get("t"))
        value = finite(row.get("dst"))
        if stamp is None or value is None:
            continue
        dst_rows.append([stamp, value])
    dst_rows.sort()
    write_rows("dst.csv", "unix,dst", dst_rows)

    imf_rows = []
    for row in series_rows("imf"):
        stamp = as_unix(row.get("t"))
        bt = finite(row.get("bt"))
        bz = finite(row.get("bz"))
        if stamp is None or bt is None or bz is None:
            continue
        imf_rows.append([stamp, bt, bz])
    imf_rows.sort()
    write_rows("imf.csv", "unix,bt,bz", imf_rows)

    grid_rows = []
    folder = ROOT / "FSOT" / "stamps" / "archive" / "grids"
    for path in sorted(folder.glob("*.json")):
        data = json.loads(path.read_text(encoding="utf-8"))
        stamp = as_unix(data.get("pulled_at"))
        if stamp is None:
            continue
        for row in data.get("rows") or []:
            lat = finite(row.get("lat"))
            lon = finite(row.get("lon"))
            pres = finite(row.get("pressure_hpa"))
            wind = finite(row.get("wind_ms"))
            if None in (lat, lon, pres, wind):
                continue
            grid_rows.append([stamp, lat, lon, pres, wind])
    write_rows("grids.csv", "unix,lat,lon,pres,wind", grid_rows)

    quake_rows = []
    quake_path = ROOT / "FSOT" / "stamps" / "archive" / "quakes.jsonl"
    for line in quake_path.read_text(encoding="utf-8").splitlines():
        if not line.strip():
            continue
        row = json.loads(line)
        stamp = as_unix(row.get("t"))
        lat = finite(row.get("lat"))
        lon = finite(row.get("lon"))
        mag = finite(row.get("mag"))
        if None in (stamp, lat, lon, mag):
            continue
        quake_rows.append([stamp, lat, lon, mag])
    write_rows("quakes.csv", "unix,lat,lon,mag", quake_rows)

    issued = datetime(2026, 11, 3, 6, 21, 24, tzinfo=timezone.utc)
    week_path = OUT / "usgs-2.5-week.geojson"
    if not week_path.exists() or week_path.stat().st_size < 1000:
        request = urllib.request.Request(USGS, headers={"User-Agent": "FSOT-octave-diagnostic/0.1"})
        with urllib.request.urlopen(request, timeout=60) as response:
            week_path.write_bytes(response.read())
        print(f"usgs_week_bytes {week_path.stat().st_size}")
    else:
        print(f"usgs_week_kept {week_path.stat().st_size}")

    doc = json.loads((ROOT / "FSOT" / "side" / "issuance-20261103T062124Z.json").read_text(encoding="utf-8"))
    centers = []
    for card in doc.get("earthquakes") or []:
        loc = card.get("location") or {}
        pred = card.get("predicted") or {}
        centers.append(
            [
                issued.timestamp(),
                float(loc["lat"]),
                float(loc["lon"]),
                float(loc["radius_km"]),
                float(pred["mag_min"]),
                1.0 if pred.get("expect_event") else 0.0,
            ]
        )
    write_rows("cells.csv", "unix,lat,lon,radius_km,mag_min,expect", centers)


if __name__ == "__main__":
    sys.path.insert(0, str(ROOT / "FSOT"))
    main()
