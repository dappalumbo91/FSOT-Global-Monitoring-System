function crunch
% Frozen-rule margins for the Earth forecast layer.
% Reads CSV copied from the monitor cache. Does not edit the pin.

root = fileparts(mfilename("fullpath"));
data = fullfile(root, "data");
out = fullfile(root, "report.txt");
fid = fopen(out, "w");
cleanup = onCleanup(@() fclose(fid));

kp = load_csv(fullfile(data, "kp.csv"));
dst = load_csv(fullfile(data, "dst.csv"));
imf = load_csv(fullfile(data, "imf.csv"));
grids = load_csv(fullfile(data, "grids.csv"));
quakes = load_csv(fullfile(data, "quakes.csv"));
cells = load_csv(fullfile(data, "cells.csv"));

fprintf(fid, "FSOT Octave crunch\n");
fprintf(fid, "Pin stays AEB2AD. These figures are diagnostics.\n\n");

fprintf(fid, "Kp measured rows %d  %s through %s\n", rows(kp), iso(kp(1, 1)), iso(kp(end, 1)));
fprintf(fid, "Dst rows %d  %s through %s\n", rows(dst), span_start(dst), span_end(dst));
fprintf(fid, "IMF rows %d  %s through %s\n", rows(imf), span_start(imf), span_end(imf));
fprintf(fid, "Grid samples %d  quake rows %d  open quake cells %d\n\n", rows(grids), rows(quakes), rows(cells));

calls = zeros(rows(kp), 1);
rise = zeros(rows(kp), 1);
for i = 1:rows(kp)
  window = kp(max(1, i - 23):i, 2);
  latest = window(end);
  rising = numel(window) >= 4 && window(end) > window(end - 3);
  rise(i) = rising;
  calls(i) = latest >= 4 || (rising && latest >= 3);
end

hit5 = find(kp(:, 2) >= 5, 1, "first");
fprintf(fid, "Kp rule\n");
fprintf(fid, "A storm call needs the latest Kp >= 4, or a rise across 4 samples with latest Kp >= 3.\n");
fprintf(fid, "Four Kp samples are 12 hours. The issued card scores 72 hours.\n");
if isempty(hit5)
  fprintf(fid, "No measured Kp >= 5 in this file.\n\n");
else
  t5 = kp(hit5, 1);
  fprintf(fid, "First measured Kp >= 5 is %.2f at %s\n", kp(hit5, 2), iso(t5));
  before = find(kp(:, 1) < t5 & kp(:, 1) >= t5 - 72 * 3600);
  storm_before = before(calls(before) == 1);
  quiet_before = before(calls(before) == 0);
  if isempty(storm_before)
    fprintf(fid, "No storm call in the 72 hours before that Kp.\n");
  else
    lead_h = (t5 - kp(storm_before(1), 1)) / 3600;
    fprintf(fid, "First storm call in that 72 hours: %s at Kp %.2f, lead %.1f hours.\n", ...
      iso(kp(storm_before(1), 1)), kp(storm_before(1), 2), lead_h);
  end
  fprintf(fid, "Quiet calls inside those 72 hours: %d. Storm calls: %d.\n", numel(quiet_before), numel(storm_before));
  quiet_while_low = quiet_before(kp(quiet_before, 2) < 3);
  fprintf(fid, "Quiet calls in that window while Kp was still under 3: %d.\n\n", numel(quiet_while_low));
end

fprintf(fid, "Dst around the Kp >= 5 row\n");
if isempty(hit5) || rows(dst) == 0
  fprintf(fid, "Dst does not cover a Kp >= 5 row.\n\n");
else
  t5 = kp(hit5, 1);
  band = dst(dst(:, 1) >= t5 - 48 * 3600 & dst(:, 1) <= t5, :);
  if rows(band) == 0
    fprintf(fid, "No Dst sample in the 48 hours before %s. Dst span ends %s.\n\n", iso(t5), span_end(dst));
  else
    [low, idx] = min(band(:, 2));
    fprintf(fid, "Lowest Dst in the 48 hours before Kp >= 5: %.1f nT at %s.\n", low, iso(band(idx, 1)));
    under30 = band(band(:, 2) <= -30, :);
    under50 = band(band(:, 2) <= -50, :);
    fprintf(fid, "Samples <= -30 nT: %d. Samples <= -50 nT: %d.\n", rows(under30), rows(under50));
    if rows(under50) > 0
      fprintf(fid, "First Dst <= -50 nT in that 48 hours: %s, %.1f hours before Kp >= 5.\n", ...
        iso(under50(1, 1)), (t5 - under50(1, 1)) / 3600);
    end
    fprintf(fid, "\n");
  end
end

fprintf(fid, "IMF Bz coverage\n");
if rows(imf) == 0
  fprintf(fid, "No IMF rows.\n\n");
else
  fprintf(fid, "IMF file runs %s through %s. Southward is Bz < 0.\n", span_start(imf), span_end(imf));
  south = imf(imf(:, 3) < 0, :);
  fprintf(fid, "Southward samples %d of %d. Most southward Bz %.2f nT.\n\n", rows(south), rows(imf), min(imf(:, 3)));
end

fprintf(fid, "Weather grid, one class per basin\n");
if rows(grids) == 0
  fprintf(fid, "No archived grid.\n\n");
else
  times = unique(grids(:, 1));
  fprintf(fid, "Snapshots %d from %s through %s.\n", numel(times), iso(times(1)), iso(times(end)));
  near_wind = sum(grids(:, 5) >= 14 & grids(:, 5) < 16);
  near_pres = sum(grids(:, 4) >= 995 & grids(:, 4) < 1005);
  fprintf(fid, "Samples with 10 m wind in [14, 16) m/s: %d of %d.\n", near_wind, rows(grids));
  fprintf(fid, "Samples with pressure in [995, 1005) hPa: %d of %d.\n", near_pres, rows(grids));
  names = {"arctic", "southern", "gulf", "tropics", "pacific", "atlantic"};
  fprintf(fid, "Basin, snapshots with a storm point, snapshots whose winning storm point moved, quiet-winner moves\n");
  for b = 1:numel(names)
    storm_snaps = 0;
    storm_moves = 0;
    quiet_moves = 0;
    last_storm = [nan, nan];
    last_quiet = [nan, nan];
    seen_storm = false;
    seen_quiet = false;
    for t = 1:numel(times)
      sample = grids(grids(:, 1) == times(t), :);
      basin_rows = sample(basin_of(sample(:, 2), sample(:, 3), names{b}), :);
      if rows(basin_rows) == 0
        continue
      end
      storm_mask = basin_rows(:, 4) < 1000 | basin_rows(:, 5) >= 15;
      quiet_mask = basin_rows(:, 4) >= 1010 & basin_rows(:, 5) < 8;
      if any(storm_mask)
        storm_snaps = storm_snaps + 1;
        part = basin_rows(storm_mask, :);
        score = part(:, 4) - 0.4 * part(:, 5);
        [~, k] = min(score);
        winner = part(k, 2:3);
        if seen_storm && (abs(winner(1) - last_storm(1)) > 0.2 || abs(winner(2) - last_storm(2)) > 0.2)
          storm_moves = storm_moves + 1;
        end
        last_storm = winner;
        seen_storm = true;
      end
      if any(quiet_mask)
        part = basin_rows(quiet_mask, :);
        score = part(:, 4) - 0.4 * part(:, 5);
        [~, k] = min(score);
        winner = part(k, 2:3);
        if seen_quiet && (abs(winner(1) - last_quiet(1)) > 0.2 || abs(winner(2) - last_quiet(2)) > 0.2)
          quiet_moves = quiet_moves + 1;
        end
        last_quiet = winner;
        seen_quiet = true;
      end
    end
    fprintf(fid, "%s, %d, %d, %d\n", names{b}, storm_snaps, storm_moves, quiet_moves);
  end
  fprintf(fid, "\n");
end

fprintf(fid, "Quake cells issued 2026-11-03T06:21:24Z\n");
fprintf(fid, "Events in the FSOT archive at or before issuance stay inputs.\n");
for c = 1:rows(cells)
  lat = cells(c, 2);
  lon = cells(c, 3);
  radius = cells(c, 4);
  mag_min = cells(c, 5);
  after = 0;
  before = 0;
  best_after = -inf;
  best_after_km = nan;
  best_after_t = nan;
  for q = 1:rows(quakes)
    km = haversine(lat, lon, quakes(q, 2), quakes(q, 3));
    if km > radius
      continue
    end
    when = quakes(q, 1);
    if when <= cells(c, 1)
      before = before + 1;
    elseif quakes(q, 4) >= mag_min
      after = after + 1;
      if quakes(q, 4) > best_after
        best_after = quakes(q, 4);
        best_after_km = km;
        best_after_t = when;
      end
    end
  end
  fprintf(fid, "Cell %.2f, %.2f radius %.1f. Inputs inside radius %d. Later M>=%.1f %d.\n", ...
    lat, lon, radius, before, mag_min, after);
  if after > 0
    fprintf(fid, "  Largest later event M%.1f at %.1f km, %s.\n", best_after, best_after_km, iso(best_after_t));
  end
end
fprintf(fid, "\nEnd.\n");
endfunction

function matrix = load_csv(path)
  if exist(path, "file") != 2
    matrix = zeros(0, 1);
    return
  end
  matrix = dlmread(path, ",", 1, 0);
  if numel(matrix) == 1 && matrix == 0
    matrix = zeros(0, 1);
  end
end

function n = rows(matrix)
  n = size(matrix, 1);
end

function text = iso(unix_time)
  text = datestr(unix_time / 86400 + datenum(1970, 1, 1), "yyyy-mm-ddTHH:MM:SSZ");
end

function text = span_start(matrix)
  if rows(matrix) == 0
    text = "none";
  else
    text = iso(matrix(1, 1));
  end
end

function text = span_end(matrix)
  if rows(matrix) == 0
    text = "none";
  else
    text = iso(matrix(end, 1));
  end
end

function mask = basin_of(lat, lon, name)
  mask = false(numel(lat), 1);
  for i = 1:numel(lat)
    mask(i) = strcmp(one_basin(lat(i), lon(i)), name);
  end
end

function name = one_basin(lat, lon)
  if lat >= 60
    name = "arctic";
  elseif lat <= -40
    name = "southern";
  elseif lat >= 18 && lat <= 32 && lon >= -98 && lon <= -80
    name = "gulf";
  elseif abs(lat) < 20
    name = "tropics";
  elseif lon <= -100 || lon >= 120
    name = "pacific";
  elseif lon >= -80 && lon <= 40
    name = "atlantic";
  else
    name = "other";
  end
end

function km = haversine(lat1, lon1, lat2, lon2)
  p1 = deg2rad(lat1);
  p2 = deg2rad(lat2);
  dp = deg2rad(lat2 - lat1);
  dl = deg2rad(lon2 - lon1);
  a = sin(dp / 2) .^ 2 + cos(p1) .* cos(p2) .* sin(dl / 2) .^ 2;
  km = 6371 * 2 * asin(min(1, sqrt(a)));
end
