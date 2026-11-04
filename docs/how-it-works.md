# How the FSOT Global Monitoring System works

The system has two parts that share a disk and do not share a process.

The globe measures. The FSOT folder predicts from those measurements and waits. A prediction counts only after a later measurement arrives.

## The globe

`fsot-global-monitor` is a Rust window. It draws the Earth with wgpu and keeps a data desk in egui. Coordinates live in `src/geo.rs`: Y is up, the north pole is +Y, and longitude 0 at latitude 0 faces +Z.

The feeds thread refreshes about every five minutes. Place lookups run only when a place is selected. A failed fetch keeps the previous series file. A real empty catalog may replace the file with an empty row list. Failure text names the agency and says the product did not load. The program does not fill a gap with an invented number.

The desk and the globe read:

- Weather and ocean grids, place hours, and a place ensemble from Open-Meteo
- USGS earthquakes
- NOAA SWPC solar wind, Kp, X-ray, plasma, protons, electrons, magnetometer, alerts, F10.7, regions, sunspots, Dst, and scales
- NASA DONKI CMEs, with SIDC CACTus when DONKI does not answer
- CPC climate indices, BOM MJO, NSIDC sea ice, NOAA Mauna Loa CO2
- NASA FIRMS active fires
- NHC and JTWC storms
- NWS tsunami bulletins and the NWS relay of VAAC ash advisories
- UNESCO IOC sea-level gauges

Static layers ship in `assets/`. Quake, storm, and warning markers come from the live feeds. A VAAC advisory is quoted as written. A high gauge is a measurement. The tsunami warning is the NWS bulletin. GDACS colors stay a pointer.

`cache/series/` holds the latest pull of each series. `cache/record/` holds the agency forecast rows the monitor itself stores, including the closed rain pair of 6.5 mm and 6.1 mm. The FSOT scripts read those files and do not write them.

## The FSOT track

`FSOT/side_by_side.py` checks the SHA-256 of `fsot_compute.py` before it imports `fsot_earth_fluid_forecast.py`. A mismatch stops the run. The forecast file is the dated class engine. It is not a millimeter rain model.

`FSOT/stamp_loop.py --watch` wakes about every five minutes.

1. Read the wind grid, the Kp table, and the USGS day feed from the monitor cache.
2. For a new earthquake card, also read the public USGS `2.5_week` catalog, and add any day-feed event whose id is missing from that file. The copy is `FSOT/stamps/archive/usgs-2.5-week.geojson`.
3. Ask the pinned functions for weather, Kp, earthquake, and volcanic cards.
4. Write a card only when it is new: a new class, a free basin slot, or a quake center outside every open cell.
5. Archive the pull under `FSOT/stamps/archive/`.
6. Score cards whose windows have closed, or whose kill condition has already arrived.
7. Append the result to `FSOT/stamps/outcomes.jsonl` and refresh `FSOT/stamps/STATUS.md`.

A miss is appended for the record. The pin is left as it was. Cards already written stay written. A later catalog that would relabel a valve does not open a second card on a cell that is already open.

Volcanic cards are emitted only when the catalog's place text or event type is volcanic. The ordinary day feed has produced none.

## How a card is scored

Weather is scored at the card's own grid point, after `valid_to`.

- A storm hits unless every sample in the window has pressure at or above 1010 hPa and wind under 8 m/s.
- A quiet call misses if any sample has pressure under 1005 hPa or wind at or above 12 m/s.
- Zero samples at that point is unmeasured.

The storm pick at issuance is pressure under 1000 hPa or wind at or above 15 m/s. The quiet-clean pick is pressure at or above 1010 hPa and wind under 8 m/s. Those thresholds stay where the pinned function put them.

Kp is scored from observed or estimated rows. A Kp of 5 or more inside the 72-hour window hits a storm call and misses a quiet call. A quiet call that reaches the end of 72 hours with no such Kp is a hit. Predicted future Kp rows are not fed into the trend. The overlapping replay in `FSOT/stamps/replay-kp.json` walks the frozen rule after the hours. It is labeled as a diagnostic.

An earthquake hits when an event after issuance, inside the radius, reaches `mag_min` before `valid_to`. If the card expected the event, that is a hit. If the card was a quiet hold, that is a miss. A loading card with no such event by `valid_to` is a miss. A quiet hold that reaches `valid_to` with no such event is a hit.

Cells whose inputs already held an event at or above the floor, such as Palekastro, are continuations. Cells whose inputs were still below the floor, such as Delta, are the harder calls. The board lists both. A rate that mixes them hides that difference.

## What the Octave pass found

`FSOT/octave/report.txt` is one numeric pass over the archive, run with GNU Octave 11.3. It did not change the pin.

On the Kp file then on disk, the rule's first storm call before Kp 6.0 on 2026-10-31T18:00Z came at 2026-10-30T21:00Z, when Kp was already 4.33. The quiet call at 2026-10-30T00:00Z missed that storm. Dst at the quiet call was +1 nT, and the later low was −37 nT after the storm call had already been made. The IMF file on disk did not cover that storm. Weather-basin storm membership was stable while the winning grid point moved. A week-long quake catalog, cut off at the 06:21Z issuance, would have labeled Nagqu released and left Palekastro, Delta, and Lower Lake loading. The issued cards were not rewritten. New quake cards after 2026-11-04T03:34Z use the week catalog.

## What is still ahead

The globe keeps measuring. The loop keeps the diary. Integration into `src/` waits until the closed calls are a body of evidence large enough to use. Weather needs a season of regimes that change before a threshold moves. Kp needs later storms that the candidate rule did not see. Earthquake calls need their 7-day windows to close, with continuation cells kept separate from cells that were still below the floor.
