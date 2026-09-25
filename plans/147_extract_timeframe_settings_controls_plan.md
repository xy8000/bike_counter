# 147 - Extract shared timeframe settings controls (Sonar duplication fix)

Status: implemented

## Report

Sonar flagged duplicated code in
[`StationDetail.tsx`](frontend/src/features/stationDetail/StationDetail.tsx:256)
(reported lines 244–278): the "Detailed statistics" header control cluster is
byte-identical to the one in
[`StationsSummary.tsx`](frontend/src/features/stationsSummary/StationsSummary.tsx:329).

## Duplicated block

Both pages render the same cluster to the right of the `Detailed statistics`
heading:

- [`TimeframeSettingsLabel`](frontend/src/features/settings/TimeframeSettingsLabel.tsx:18)
  with the current settings,
- a `Button` (aria-label `Calculation settings`) that opens the dialog, and
- the [`SettingsDialog`](frontend/src/features/settings/SettingsDialog.tsx:34)
  bound to the page-local `settingsOpen` `useState`.

The markup **and** the `settingsOpen` state were copied verbatim, so a change to
one copy (a new control, a label tweak, an extra prop) silently drifts from the
other — exactly the maintenance hazard the section helpers in
[`sections.tsx`](frontend/src/features/stationDetail/sections.tsx:1) were
introduced to avoid for the section bodies.

## Fix

Extract the cluster into a single
[`TimeframeSettingsControls`](frontend/src/features/settings/TimeframeSettingsControls.tsx:1)
component that owns the dialog's open state and renders the label + button +
dialog. Both pages replace their copy with
`<TimeframeSettingsControls settings={settings} />` and drop the now-unused
`settingsOpen` state and the `Button` / `SlidersHorizontal` / `SettingsDialog` /
`TimeframeSettingsLabel` imports.

The markup is preserved exactly (same `aria-label`, `title` and button text), so
the existing `StationDetail`/`StationsSummary` tests — which click the
`Calculation settings` button and expect the `Bike-Trends settings` dialog —
keep passing unchanged.

## Definition of done

- [x] Plan file created
- [x] `TimeframeSettingsControls` component added (owns the dialog open state)
- [x] `StationDetail` renders the shared component
- [x] `StationsSummary` renders the shared component
- [x] Unit test for the new component
- [x] `make check` green (frontend Prettier included)
- [x] `make test-unit-coverage` green (whole-`src` line coverage ≥ 80 %)
- [x] `make test-playwright` green (74 passed)
