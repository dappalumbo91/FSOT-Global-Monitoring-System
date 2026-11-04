# Background FSOT track

The public description of this system is `README.md` and `docs/how-it-works.md`. This file is the operating rule for the machine that runs the monitor.

The release monitor keeps running. This folder is not part of the Rust binary. Nothing here is wired into `src/` until a held-out comparison is ready.

## Engine

- Verified engine: `C:\Users\damia\Desktop\FSOT-2.1-Lean`
- Numerical authority: `vendor/fsot_compute.py`
- Pin AEB2AD, SHA-256 `AEB2ADAD6E80F487772C5DF90A2E3DDA71624AB831A6A83B94AB471AC9AAC170`, checked 2026-11-03
- C++ port on the same pin: `C:\Users\damia\Desktop\FSOT-2.1-Cpp`
- The August freeze and the genetics checkout stay out. Do not mix pin D1D38A into this track.
- Confirmed held-out accuracy is 0.

## Goal

A usable product that issues FSOT calls for the systems on this monitor and scores those calls against the later measurement. The claim being built is that those calls are accurate enough to use. It is not a contest with Open-Meteo.

The diary is `FSOT/stamps/STATUS.md`. The loop appends the ledger, the outcomes, and the misses. A year of that record does not need a separate notebook. The loop keeps writing while this machine is on and `stamp_loop.py --watch` is running.

## Comparison that counts

An FSOT number counts only when it is written before the hour closes. After the hour, the record can hold three values for that same hour: the agency forecast stored ahead of time, the agency's later value, and the FSOT number. The first closed pair already on the monitor is Open-Meteo rain at 2026-11-02T18: stored forecast 6.5 mm, later value 6.1 mm. FSOT did not produce either number.

A formula adjusted on the same hours it is scored against stays a fit. The count that matters is the held-out one.

## While the monitor is running

- Do not stop `target\release\fsot-global-monitor.exe`.
- Do not write `cache/record` or `cache/series` from this folder. The monitor owns those files.
- A historical pull uses its own copies and stays off the monitor's request pace.

## Stamp loop

- `FSOT/stamp_loop.py` reads the monitor cache and writes `FSOT/stamps/`. A cache file caught mid-write is retried. One failed read is logged, and the loop keeps going.
- Each new series pull is archived here. A card is counted once while its window is open. A later pull that repeats that class and place is carried. A second copy of a card that is still open is a duplicate and is not a second test. One weather class per basin stays open. A move to another grid point in that same basin is carried with the first card. One earthquake cell stays open inside its radius. A later catalog that would relabel that valve does not open a second test. The first card stands.
- A new quake card is built from the public USGS `2.5_week` feed, plus day-feed events whose ids are not in that file yet. The copy lives at `FSOT/stamps/archive/usgs-2.5-week.geojson` and is refreshed when it is older than 30 minutes. Cards written before that rule stay as written. The monitor day feed still scores the card.
- Confirmed held-out stays 0 until a call written before its window closes is scored against a later measurement.
- A replay of the frozen Kp rule on rows already in the file is a diagnostic. It was computed after those hours, so it is not held-out.
- A miss is logged. The pin is not edited to fit the miss.
