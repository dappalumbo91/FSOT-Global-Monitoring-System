#!/usr/bin/env python3
"""Valve state at the four issued cells if the USGS week feed is the catalog.

Events after 2026-11-03T06:21:24Z stay out. This does not write a new card.
"""

from __future__ import annotations

import json
import os
import sys
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
LEAN = Path(os.environ.get("FSOT_LEAN_VENDOR", r"C:\Users\damia\Desktop\FSOT-2.1-Lean\vendor"))
WEEK = ROOT / "FSOT" / "octave" / "data" / "usgs-2.5-week.geojson"
ISSUED = datetime(2026, 11, 3, 6, 21, 24, tzinfo=timezone.utc)


def main() -> None:
    sys.path.insert(0, str(LEAN))
    import fsot_earth_fluid_forecast as earth

    doc = json.loads((ROOT / "FSOT" / "side" / "issuance-20261103T062124Z.json").read_text(encoding="utf-8"))
    week = json.loads(WEEK.read_text(encoding="utf-8"))
    issued_ms = int(ISSUED.timestamp() * 1000)
    half_ms = int(earth.forecast_horizon_days() * 0.5 * 86400 * 1000)
    events = []
    for feature in week.get("features") or []:
        props = feature.get("properties") or {}
        coords = (feature.get("geometry") or {}).get("coordinates") or []
        if len(coords) < 2 or props.get("mag") is None or props.get("time") is None:
            continue
        if int(props["time"]) > issued_ms:
            continue
        events.append(
            {
                "lat": float(coords[1]),
                "lon": float(coords[0]),
                "mag": float(props["mag"]),
                "time": int(props["time"]),
                "place": props.get("place") or "",
            }
        )
    print(f"week_events_at_or_before_issue {len(events)} half_days {half_ms / 86400000:.2f}")
    for card in doc.get("earthquakes") or []:
        loc = card["location"]
        pred = card["predicted"]
        members = [
            event
            for event in events
            if earth.haversine_km(float(loc["lat"]), float(loc["lon"]), event["lat"], event["lon"]) <= float(loc["radius_km"])
        ]
        state = earth.valve_state(members, now_ms=issued_ms, half_ms=half_ms)
        recent = [event for event in members if event["time"] >= issued_ms - half_ms]
        prior = [event for event in members if issued_ms - 2 * half_ms <= event["time"] < issued_ms - half_ms]
        print(
            f"{card['id']} issued_state {pred.get('valve_state')} week_state {state} "
            f"members {len(members)} recent {len(recent)} prior {len(prior)} "
            f"max {max((event['mag'] for event in members), default=0):.1f} "
            f"place {loc.get('name')}"
        )


if __name__ == "__main__":
    main()
