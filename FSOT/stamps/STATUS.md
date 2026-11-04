# FSOT stamp board

Updated 2026-11-04T17:13:41Z.
Scored calls written before the measurement: 10.
This board is the diary. The monitor cache was not written.

## Goal

Usable forward calls from the pinned FSOT engine.
Open this file for the score. The loop appends a hit or a miss when a window closes.
Cards already written stay as written. A new quake card uses the USGS 7-day catalog.

## This pull

Action `carry` file `issuance-20261104T155334Z.json`.
wind-grid pulled `2026-11-04  17:11:14 UTC`.
kp pulled `2026-11-04  17:11:51 UTC`.
quakes pulled `2026-11-04  17:11:13 UTC`.

## Counted calls

Counted issuances: 20. Carried pulls: 393.
Hits 10. Misses 0. Unmeasured 0.
Duplicate cards left out of the score: 43. Misses logged: 0.
A carried pull repeats an open card and does not add a test.
A later card with the same class and place as a card still open is a duplicate.
A hit or a miss is a call this loop wrote before the measurement.

## Kp replay diagnostic

The frozen Kp rule was walked across the measured rows already in the cache.
Those calls were computed after the hours, so this replay is not held-out.
Measured Kp rows 82 from `2026-10-27T00:00:00Z` through `2026-11-04T15:00:00Z`. Maximum Kp 6.0.
Overlapping 3-hour stamps: 21 hits, 25 misses, 36 still open, of 46 resolved.
Among the resolved overlapping stamps, quiet calls 39 and storm calls 7.
Non-overlapping 72-hour steps: 1 hits and 1 misses across 2 windows.

## Series on the clock

The pinned forecast functions take the wind grid, the Kp table, and USGS earthquakes.
A new quake card is built from the 7-day catalog, plus day-feed ids that file does not yet have.
The day feed still scores the card. Wind-hours are the agency look-ahead stored beside a weather card.
The other series stay archived by the monitor and have no forecast function on this pin.

air, alerts, ao, cme, co2, dst, electrons, ensemble, f107, fires, flares, gauges, imf, mag, mjo, nao, ocean-grid, ocean-hours, oni, protons, regions, rtsw, scales, sea-ice, storms, sunspots, vaac, xray

## Still open

- `FCAST-WX-20261104T0654-04` weather through 2026-11-05T06:54:49Z NDBC -40,160 (pacific) valve `steady`.
- `FCAST-WX-20261104T0934-01` weather through 2026-11-05T09:34:57Z NDBC -60,-140 (southern) valve `loading_suction`.
- `FCAST-WX-20261104T0934-02` weather through 2026-11-05T09:34:57Z NDBC 60,-180 (arctic) valve `loading_suction`.
- `FCAST-WX-20261104T0934-03` weather through 2026-11-05T09:34:57Z NDBC 60,-40 (atlantic) valve `loading_suction`.
- `FCAST-WX-20261104T0934-04` weather through 2026-11-05T09:34:57Z NDBC 20,-80 (gulf) valve `steady`.
- `FCAST-WX-20261104T0934-05` weather through 2026-11-05T09:34:57Z NDBC 0,80 (tropics) valve `steady`.
- `FCAST-WX-20261104T1004-05` weather through 2026-11-05T10:04:59Z NDBC -20,-40 (atlantic) valve `steady`.
- `FCAST-WX-20261104T1553-03` weather through 2026-11-05T15:53:34Z NDBC 40,-180 (pacific) valve `loading_suction`.
- `FCAST-SOL-20261103T0621-01` solar through 2026-11-06T06:21:24Z Earth magnetosphere (planetary Kp) valve `loading_suction`.
- `FCAST-SOL-20261103T0902-01` solar through 2026-11-06T09:02:02Z Earth magnetosphere (planetary Kp) valve `steady`.
- `FCAST-EQ-20261103T0621-02` earthquake through 2026-11-10T06:21:24Z 193 km ENE of Nagqu, China valve `loading_suction`.
- `FCAST-EQ-20261103T0621-04` earthquake through 2026-11-10T06:21:24Z 8 km E of Lower Lake, CA valve `loading_suction`.
- `FCAST-EQ-20261103T1735-02` earthquake through 2026-11-10T17:35:46Z 52 km WNW of Ninilchik, Alaska valve `loading_suction`.
- `FCAST-EQ-20261103T2309-01` earthquake through 2026-11-10T23:09:39Z 91 km ESE of Kyzyl-Eshme, Kyrgyzstan valve `loading_suction`.
- `FCAST-EQ-20261104T0119-03` earthquake through 2026-11-11T01:19:45Z 3 km SE of Meno, Oklahoma valve `loading_suction`.
- `FCAST-EQ-20261104T0317-04` earthquake through 2026-11-11T03:17:49Z 54 km WNW of Nanwalek, Alaska valve `loading_suction`.
- `FCAST-EQ-20261104T0327-04` earthquake through 2026-11-11T03:27:50Z 31 km SSE of Skwentna, Alaska valve `loading_suction`.
- `FCAST-EQ-20261104T0334-01` earthquake through 2026-11-11T03:34:36Z southeast of Easter Island valve `released`.
- `FCAST-EQ-20261104T0334-02` earthquake through 2026-11-11T03:34:36Z 43 km ESE of Port-Olry, Vanuatu valve `released`.
- `FCAST-EQ-20261104T0334-03` earthquake through 2026-11-11T03:34:36Z 165 km NW of Port-Olry, Vanuatu valve `post_poof_aftershock`.
- `FCAST-EQ-20261104T0334-04` earthquake through 2026-11-11T03:34:36Z 110 km ENE of Barcelona, Philippines valve `released`.
- `FCAST-EQ-20261104T0334-05` earthquake through 2026-11-11T03:34:36Z 58 km SSE of Shima, Japan valve `post_poof_aftershock`.
- `FCAST-EQ-20261104T0334-06` earthquake through 2026-11-11T03:34:36Z 271 km WSW of Tual, Indonesia valve `released`.
- `FCAST-EQ-20261104T0334-07` earthquake through 2026-11-11T03:34:36Z 82 km NE of Kiunga, Papua New Guinea valve `released`.
- `FCAST-EQ-20261104T0334-08` earthquake through 2026-11-11T03:34:36Z 5 km ESE of Asahi, Japan valve `released`.
- `FCAST-EQ-20261104T1533-08` earthquake through 2026-11-11T15:33:32Z 24 km NNW of Rioblanco, Colombia valve `loading_suction`.

## Open scoring

No open card resolved on this pass.
