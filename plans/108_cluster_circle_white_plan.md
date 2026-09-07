# 108 - Cluster-circle text is white; the ring follows the foreground

Status: implemented

## Problem

The numbered cluster circles added by
[`107_map_station_grouping_plan.md`](107_map_station_grouping_plan.md) use the
theme tokens for their two "chromatic" properties, and both pick the wrong end in
dark mode:

- The station count renders with `text-primary-foreground` (the colour that
  contrasts against `--primary`), which is near-white in light mode but
  **near-black** (`--primary-foreground: oklch(0.129 0.042 264.695)`) in dark
  mode, so the count number shows up black on the bright primary-green circle.
- The surrounding ring is `border-background`, which follows the **app**
  background (white in light mode, near-black in dark mode) — the opposite of the
  requested ring, which must contrast with the **basemap** (dark basemap ↔ white
  ring, light basemap ↔ black ring).

## Goal

On every map cluster circle: render the count number **white in both colour
schemes**, and give the ring the **foreground** colour — black on the light
basemap, white on the dark basemap — instead of the app-background colour.

## Approach

The cluster circle is a single `<button>` in
[`MapView.tsx`](frontend/src/features/map/MapView.tsx:145):

```
className="station-cluster … border-2 border-background bg-primary … text-primary-foreground …"
```

Two class swaps, both presentation-only:

1. Text colour `text-primary-foreground` → `text-white`, so the count is white on
   the emerald `bg-primary` in both schemes (light-mode `primary-foreground` was
   effectively white already, so there is no light-mode regression).
2. Ring colour `border-background` → `border-foreground`. `--foreground` is
   near-black in light mode and near-white in dark mode, i.e. the ring contrasts
   with the map (black ring on the light basemap, white ring on the dark basemap)
   as requested.

The change does **not** touch the `--primary-foreground`/`--background` tokens
(that would recolour every `bg-primary` component / page backdrop in dark mode),
does not change the circle background (`bg-primary`), sizing, the
`station-cluster` e2e locator, `data-count`, or any behaviour.

## Files touched

| File | Change |
|---|---|
| [`frontend/src/features/map/MapView.tsx`](frontend/src/features/map/MapView.tsx:145) | Cluster `<button>`: `text-primary-foreground` → `text-white` and `border-background` → `border-foreground` |
| [`plans/README.md`](plans/README.md) | Register this plan as the current plan |

No e2e assertion checks the circle text or border colour (the specs only assert
the circle is visible, its `data-count`/text value and the click-to-zoom
behaviour), so the Playwright suite is unaffected; `make check` (frontend
`prettier --check`) covers the one-line class change.

## Gates

- `make check` (prettier + cargo fmt/clippy + audit) — frontend-only class swap.

## Definition of done

- [x] Cluster-circle count renders white in both light and dark scheme
- [x] No other element recoloured; selectors/locators unchanged
- [x] `make check` green
