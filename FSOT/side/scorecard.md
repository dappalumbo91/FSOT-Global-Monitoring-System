# Side-by-side accuracy

Issued 2026-11-03T06:21:24Z.
Engine pin AEB2AD. SHA-256 `AEB2ADAD6E80F487772C5DF90A2E3DDA71624AB831A6A83B94AB471AC9AAC170`.
This file is the background track. The live monitor was not hooked up and its cache was not written.

## Confirmed held-out

Confirmed held-out accuracy is 0.
An FSOT number counts only after it is written before the window closes and a later agency value arrives.
The statements in this file are the first open issuances. Their windows are still ahead.

## Closed hours on the monitor

- `rain` hour `2026-11-02T18`: stored forecast 6.5 , later value 6.1 , absolute difference 0.4 . Source `open-meteo-hour`. Written `2026-11-03T02:26:48Z`.

On rain hour `2026-11-02T18`, both numbers are Open-Meteo. The stored forecast is 6.5 mm and the later value is 6.1 mm. The absolute difference is 0.4 mm.
FSOT issued no number for that hour. The FSOT error on that hour is empty.
The result line does not carry a latitude or longitude, so this card does not add one.

## What the engine issued from the same cache

The pinned Earth-fluid functions emit a sector or a magnitude class. They do not emit a rain millimeter, a wave height, or a wind-speed hour.
`pres_now`, the wind value in the gust slot, and `kp_now` are measured inputs copied onto the card.
The wind value is Open-Meteo 10 m wind speed in m/s, passed through the function's gust field.
Weather cards keep the function's NDBC wording. The points are Open-Meteo grid samples.

### Weather, next 24 hours

Grid rows 126. Usable points 126. Storm points 21. Quiet points 70. Points outside both envelopes 35.
Cards issued: 6.

- `FCAST-WX-20261103T0621-01` storm_sector at -60°, -80° basin `southern`. Window through `2026-11-04T06:21:24+00:00`.
  Measured input: pressure 963.6 hPa, 10 m wind 14.7 m/s.
  Class hold: pressure cap 1010 hPa, gust floor 8 m/s. NDBC -60,-80 next 24h: pressure stays ≥1010 hPa AND gust <8 m/s
  Open-Meteo hours at this point inside the window: 23. Lowest stored pressure 956.4 hPa. Highest stored wind 18.8 m/s. Sum of stored hourly precipitation 4.10 mm. Those three figures are the agency forecast. They are not a score.
- `FCAST-WX-20261103T0621-02` storm_sector at 60°, -180° basin `arctic`. Window through `2026-11-04T06:21:24+00:00`.
  Measured input: pressure 977.2 hPa, 10 m wind 12.2 m/s.
  Class hold: pressure cap 1010 hPa, gust floor 8 m/s. NDBC 60,-180 next 24h: pressure stays ≥1010 hPa AND gust <8 m/s
  Open-Meteo hours at this point inside the window: 23. Lowest stored pressure 976.2 hPa. Highest stored wind 12.7 m/s. Sum of stored hourly precipitation 2.70 mm. Those three figures are the agency forecast. They are not a score.
- `FCAST-WX-20261103T0621-03` storm_sector at -40°, 120° basin `pacific`. Window through `2026-11-04T06:21:24+00:00`.
  Measured input: pressure 1014.1 hPa, 10 m wind 15.2 m/s.
  Class hold: pressure cap 1010 hPa, gust floor 8 m/s. NDBC -40,120 next 24h: pressure stays ≥1010 hPa AND gust <8 m/s
  Open-Meteo hours at this point inside the window: 23. Lowest stored pressure 1014.6 hPa. Highest stored wind 16.0 m/s. Sum of stored hourly precipitation 0.10 mm. Those three figures are the agency forecast. They are not a score.
- `FCAST-WX-20261103T0621-04` quiet_sector at -40°, -80° basin `atlantic`. Window through `2026-11-04T06:21:24+00:00`.
  Measured input: pressure 1010.8 hPa, 10 m wind 7.4 m/s.
  Class hold: pressure cap 1035 hPa, gust floor 0 m/s. NDBC -40,-80 next 24h: pressure drops below 1005 hPa or gust ≥12 m/s
  Open-Meteo hours at this point inside the window: 23. Lowest stored pressure 1010.7 hPa. Highest stored wind 9.9 m/s. Sum of stored hourly precipitation 0.20 mm. Those three figures are the agency forecast. They are not a score.
- `FCAST-WX-20261103T0621-05` quiet_sector at 0°, 0° basin `tropics`. Window through `2026-11-04T06:21:24+00:00`.
  Measured input: pressure 1011.9 hPa, 10 m wind 6.6 m/s.
  Class hold: pressure cap 1035 hPa, gust floor 0 m/s. NDBC 0,0 next 24h: pressure drops below 1005 hPa or gust ≥12 m/s
  Open-Meteo hours at this point inside the window: 23. Lowest stored pressure 1009.4 hPa. Highest stored wind 7.0 m/s. Sum of stored hourly precipitation 4.30 mm. Those three figures are the agency forecast. They are not a score.
- `FCAST-WX-20261103T0621-06` quiet_sector at 20°, -80° basin `gulf`. Window through `2026-11-04T06:21:24+00:00`.
  Measured input: pressure 1012.2 hPa, 10 m wind 5.8 m/s.
  Class hold: pressure cap 1035 hPa, gust floor 0 m/s. NDBC 20,-80 next 24h: pressure drops below 1005 hPa or gust ≥12 m/s
  Open-Meteo hours at this point inside the window: 23. Lowest stored pressure 1011.2 hPa. Highest stored wind 8.1 m/s. Sum of stored hourly precipitation 11.40 mm. Those three figures are the agency forecast. They are not a score.

### Planetary Kp, next 72 hours

Measured Kp rows at or before issuance: 59. Latest measured Kp 1.33 at `2026-11-03T06:00:00Z`.
Null Kp rows left out: 0. Predicted rows left out of the input: 17.
Card: `FCAST-SOL-20261103T0621-01` quiet_sector. Expect Kp ≥ 5: False.
Kill: NOAA SWPC planetary Kp does reach 5 or above in the next 72 hours
NOAA predicted Kp inside the same 72 hours peaks at 3.67 on `2026-11-05T06:00:00Z`. That peak is the agency forecast. It is not a score.

### Earthquakes, calendar window from the engine

USGS rows used: 41. Skipped: 0. Catalog span `2026-11-02T06:52:59Z` through `2026-11-03T04:04:13Z`.
Horizon 7 days. Cell radius 39.1 km.
The series is the USGS day feed. The engine's prior half-window is mostly empty on a one-day catalog, so a loading label can come from that short span.
Cards issued: 4.

- `FCAST-EQ-20261103T0621-01` loading_suction near 81 km SSE of Palekastro, Greece. 34.45°, 26.48°, radius 39.1 km. Expect an event: True. Magnitude floor 4.5. Recent count 2, recent max M5.3.
  No USGS M≥4.5 inside 39 km of (34.449,26.481) between 2026-11-03 and 2026-11-10
- `FCAST-EQ-20261103T0621-02` loading_suction near 193 km ENE of Nagqu, China. 32.33°, 93.77°, radius 39.1 km. Expect an event: True. Magnitude floor 4.5. Recent count 2, recent max M4.7.
  No USGS M≥4.5 inside 39 km of (32.327,93.766) between 2026-11-03 and 2026-11-10
- `FCAST-EQ-20261103T0621-03` loading_suction near 5 km NE of Delta, B.C., MX. 32.41°, -115.17°, radius 39.1 km. Expect an event: True. Magnitude floor 4.5. Recent count 4, recent max M3.5.
  No USGS M≥4.5 inside 39 km of (32.412,-115.165) between 2026-11-03 and 2026-11-10
- `FCAST-EQ-20261103T0621-04` loading_suction near 8 km E of Lower Lake, CA. 38.88°, -122.62°, radius 39.1 km. Expect an event: True. Magnitude floor 4.5. Recent count 3, recent max M2.8.
  No USGS M≥4.5 inside 39 km of (38.879,-122.622) between 2026-11-03 and 2026-11-10

### Volcanic class

Cards issued: 0. The function keeps a USGS row when the place text contains “volcan” or the event type is volcanic. This day feed has no separate type field.

## Agency forecasts still open on the monitor

These rows are the monitor's stored look-ahead. They are not FSOT numbers.

- `wind` hour `2026-11-03T12` value 19.81 at -60.00°, -180.00°.
- `wave` hour `2026-11-03T15` value 7.18 at -59.96°, -90.04°.
- `kp` hour `2026-11-05T00` value 2.67 at 0.00°, 0.00°.
- `wind` hour `2026-11-03T14` value 18.79 at -60.00°, -79.98°.
- `wave` hour `2026-11-03T14` value 7.02 at -59.96°, -90.04°.
- `rain` hour `2026-11-03T20` value 11.40 at -39.97°, 140.03°.
- `kp` hour `2026-11-05T06` value 3.67 at 0.00°, 0.00°.
- `wind` hour `2026-11-04T02` value 19.03 at -60.00°, -159.95°.

## Series read

- wind-grid pulled `2026-11-03  06:17:52 UTC` source `Open-Meteo`.
- wind-hours pulled `2026-11-03  06:17:52 UTC` source `Open-Meteo`.
- kp pulled `2026-11-03  06:17:53 UTC` source `NOAA SWPC planetary K index`.
- quakes pulled `2026-11-03  06:17:51 UTC` source `USGS`.

Full engine cards, including fold fields, are in the issuance JSON beside this file.
