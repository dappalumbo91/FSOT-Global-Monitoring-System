# FSOT Global Monitoring System

An interactive Earth that reads public measurements, and a separate FSOT track that writes a forecast before the window and scores it against the later measurement.

This repository is in testing. The globe is the measuring instrument. FSOT is not drawn on the globe yet. The score lives in [`FSOT/stamps/STATUS.md`](FSOT/stamps/STATUS.md).

## What it is

The Rust program `fsot-global-monitor` draws a labeled Earth and pulls agency feeds. Every live source is reachable with no account, no paywall, and no key wall. NASA DONKI is called with the public `DEMO_KEY` string.

The `FSOT/` folder runs the pinned engine from [FSOT-2.1-Lean](https://github.com/dappalumbo91/FSOT-2.1-Lean) beside those measurements. The numerical authority is `vendor/fsot_compute.py` in that repository. Pin AEB2AD is the SHA-256 prefix of that file:

`AEB2ADAD6E80F487772C5DF90A2E3DDA71624AB831A6A83B94AB471AC9AAC170`

The law is `S = K(T1 + T2 + T3)`, built from five seeds and zero free parameters. The C++ port on the same pin is [FSOT-2.1-Cpp](https://github.com/dappalumbo91/FSOT-2.1-Cpp). This monitor does not use pin D1D38A.

A longer description is in [`docs/how-it-works.md`](docs/how-it-works.md). The operating rules for the running machine are in [`FSOT/PROTOCOL.md`](FSOT/PROTOCOL.md).

## What a prediction is

An FSOT card is a class call written before its window. The pinned forecast functions emit three kinds:

- Weather: storm or quiet for one marine basin, for 24 hours. The location name says NDBC because that is the function's label. On this monitor the pressure and wind are Open-Meteo values at a grid point.
- Planetary Kp: whether Kp reaches 5 within 72 hours.
- Earthquake: one event at or above a magnitude floor inside 39.1 km, over about 7 days. A loading cell expects M≥4.5. A released, steady, or post-POOF cell is a quiet hold, killed only by another M≥5.

FSOT does not emit rain millimeters, wave height, or an hourly wind speed. `pres_now`, the wind placed in the gust slot, and `kp_now` are measured inputs copied onto the card.

A card is counted once while its window is open. A later copy is a duplicate. One weather class per basin stays open. One earthquake cell stays open inside its radius. The first card keeps that cell.

## Score so far

At 2026-11-04T17:13:41Z the board showed 10 hits and 0 misses. That is ten closed calls. The Lean rigor audit's confirmed held-out count is 0, and that count is a separate claim from this board.

The ten calls, each written before its window:

- `FCAST-EQ-20261103T0621-01`, Palekastro, Greece. USGS `us7000tv51`, M4.9, 16 km from the center. The cell's inputs already included M5.3.
- `FCAST-EQ-20261103T0621-03`, Delta, B.C., Mexico. USGS `ew1793730460`, M5.0, about 1.6 km from the center. The inputs at issuance reached M3.46.
- Six weather cards from 2026-11-03T06:21Z. Southern pressure stayed 957–972 hPa. Arctic pressure stayed 977–980 hPa. Pacific pressure stayed 1014–1020 hPa with wind 10.9–15.5 m/s. Atlantic, tropics, and gulf never reached 1005 hPa or 12 m/s.
- `FCAST-WX-20261103T0653-04`, quiet Pacific at −20°, 140°. Pressure stayed 1010.3–1014.2 hPa and wind stayed 0.25–4.67 m/s.
- `FCAST-WX-20261103T0933-03`, Atlantic storm at 60°, −40°. Pressure ran 1002–1010 hPa and wind ran 2.9–19.6 m/s.

Open-Meteo stored 6.5 mm for the hour 2026-11-02T18 and later reported 6.1 mm for that same hour. The difference is 0.4 mm. FSOT issued no number for that hour.

A replay of the frozen Kp rule across rows already in the file is a diagnostic computed after those hours. It is not part of the 10.

## Run the globe

This tree was built with Rust 1.95 on Windows. The window needs a Vulkan GPU.

```powershell
cargo run --release
```

`cargo run -- --check` loads the feeds and exits. The release binary is `target/release/fsot-global-monitor.exe`. Start it with the project directory as the working directory. The program writes its cache under `cache/`. That cache is local and is not part of this repository.

Imagery and static layers are in `assets/`: NASA Blue Marble day, NASA Earth at night, Natural Earth city names, Bird 2002 plate boundaries, Smithsonian volcanoes, and a coarse elevation grid.

## Run the FSOT track

The stamp loop reads the monitor cache and writes only under `FSOT/`. It needs Python 3 and a checkout of FSOT-2.1-Lean. On this machine the vendor directory is `C:\Users\damia\Desktop\FSOT-2.1-Lean\vendor`. Set `FSOT_LEAN_VENDOR` to point somewhere else.

```powershell
Set-Location FSOT
python stamp_loop.py --watch
```

`python stamp_loop.py` runs one pass and prints a status summary. `python side_by_side.py` writes one issuance under `FSOT/side/` after it checks the pin.

The Octave numeric bench is `FSOT/octave/`. Its report is a diagnostic. It does not move the pin.

## Layout

| Path | Role |
| --- | --- |
| `src/` | Globe, feeds, desk, and series store |
| `assets/` | Imagery and static geography |
| `FSOT/stamp_loop.py` | Archive each pull, issue new cards, score closed windows |
| `FSOT/side/` | Issued cards and the first scorecard |
| `FSOT/stamps/` | Ledger, outcomes, and the board |
| `docs/how-it-works.md` | How the two parts fit together |

## Limits

The comparison that counts is a call written before the window, scored on a later measurement. A formula adjusted on the same hours it is scored against stays a fit. The pin is not edited to swallow a miss. A short distance between two measurements is a shared neighborhood. Joint lines on the globe do not say that FSOT predicts.
