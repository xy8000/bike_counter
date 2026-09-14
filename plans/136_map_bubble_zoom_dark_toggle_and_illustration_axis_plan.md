# 136 - Map bubble zoom, dark-mode exclude toggle and illustration axis fix

Status: implemented

## Problem

Three small frontend regressions:

1. Selecting a counting station on the map flies to street level at `zoom: 15`.
   The user wants the zoom-in to be increased by 100 %. In map terms a zoom
   level is logarithmic — each additional level doubles the on-screen scale —
   so a 100 % increase equals one zoom level: `15` → `16`.

2. The "Exclude new stations from trends" switch (in the settings dialog) is
   invisible in dark mode when checked: the track turns green but the thumb
   disappears. The thumb uses `dark:bg-input/30`; in the dark palette
   `--input` is already `oklch(1 0 0 / 15%)`, so at 30 % opacity the thumb is
   effectively transparent against the checked `bg-primary` (green) track.

3. The decorative chart in the same settings dialog draws single-letter
   x-axis labels `J` and `A` (first/last month of `Jan–Aug`). The user wants
   those removed.

## Fixes

### 1. Increase station-selection zoom by 100 %

[`frontend/src/features/map/MapPage.tsx`](../frontend/src/features/map/MapPage.tsx:117)

```ts
map.flyTo({ center: [longitude, latitude], zoom: 15 })
```

→

```ts
map.flyTo({ center: [longitude, latitude], zoom: 16 })
```

Scope note: only the in-app selection path is changed. The deep-link
"Find on map" arrival uses `fitBounds` (not `zoom: 15`) and stays instant; the
non-interactive detail preview keeps its own `zoom: 15` because it is not the
"zoom in" action being reported.

### 2. Make the switch thumb visible in dark mode

[`frontend/src/components/ui/switch.tsx`](../frontend/src/components/ui/switch.tsx:19)

Thumb class `dark:bg-input/30` → `dark:bg-foreground`, matching the standard
shadcn switch thumb. This gives a near-white knob in dark mode, visible on both
the checked green track and the unchecked dark track.

### 3. Remove the J/A x-axis labels from the illustration

[`frontend/src/features/settings/TrendSettingsIllustration.tsx`](../frontend/src/features/settings/TrendSettingsIllustration.tsx:4)

- Delete the `MONTHS` constant.
- Delete the x-axis label row (`<div className="mt-1 flex justify-between …">`
  rendering `MONTHS[0]` and `MONTHS[MONTHS.length - 1]`).

[`frontend/src/features/settings/TrendSettingsIllustration.test.tsx`](../frontend/src/features/settings/TrendSettingsIllustration.test.tsx:16)

- Remove the `screen.getByText('J')` / `screen.getByText('A')` assertions and
  the "Month labels …" comment.

## Out of scope

- The real time-series/monthly charts keep their current axis labels
  (`Jan`/`Feb`/… via `timeframes.ts` and `MonthlyBarChart.tsx`); they are not the
  source of the `J`/`A` letters.
- No backend/BFF changes.

## Gate results

- `make test-unit` — green (87 files, 553 tests).
- `make test-playwright` — green (74 passed).
- `make check` — `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`
  and `prettier --check` all pass. `cargo audit` fails on **RUSTSEC-2026-0285**
  (rustls 0.23.43, published 2026-09-14, fix ≥ 0.23.45) — a backend dependency
  advisory unrelated to these frontend changes and outside this branch's scope.

## Definition of done

- [x] New branch `fix/map-bubble-zoom-and-dark-toggle` created from `main`
- [x] `MapPage.selectStation` flies to `zoom: 16`
- [x] `Switch` thumb uses `dark:bg-foreground` and stays visible in dark mode
- [x] `TrendSettingsIllustration` no longer renders `J` / `A` x-axis labels
- [x] `TrendSettingsIllustration.test.tsx` updated accordingly
- [x] `make check` — fmt/clippy/prettier green; cargo audit fails on an unrelated
      rustls advisory (RUSTSEC-2026-0285)
- [x] `make test-unit` green
- [x] `make test-playwright` green
