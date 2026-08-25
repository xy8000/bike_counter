# 26 - Sidebar overlay + find-on-map popup consistency plan

Status: implemented

## Problem

Two frontend regressions in [`frontend/src/App.tsx`](../frontend/src/App.tsx:1) and
[`frontend/src/index.css`](../frontend/src/index.css:1):

1. The header `topbar` renders a `◀` toggle button (the
   [`sidebarCollapsed` block](../frontend/src/App.tsx:325)) that reveals the
   sidebar. This button does not belong in the header — the sidebar should be an
   extendable overlay, with its expand and collapse controls inside the sidebar
   itself (Komoot style).
2. "Find on map" builds a one-off `L.popup()` with
   `<strong>…</strong>` content via
   [`focusStation`](../frontend/src/App.tsx:274), which renders differently from
   clicking a map marker (whose [`Popup`](../frontend/src/App.tsx:392) shows the
   plain station name and is anchored to the marker icon).

## Goal

- Remove the header sidebar toggle.
- Turn the sidebar into a Komoot-style overlay that floats over the map; the map
  stays full width underneath. Collapsed, the sidebar is a slim vertical edge
  with a `>` expand button; expanded, it is the full panel with a `<` collapse
  button.
- "Find on map" flies to the station (zoom ~15) and then opens a popup identical
  to a map-marker click: plain station name, anchored above the marker.

## Decisions (clarified)

1. **Overlay, not a layout rail** — the sidebar is absolutely positioned over the
   map; the map keeps its full width.
2. **Controls live in the sidebar** — `>` expands, `<` collapses.
3. **Popup parity** — the find-on-map popup matches the marker popups exactly:
   plain (HTML-escaped) station name, anchored above the marker using the default
   marker icon's popup anchor.
4. **Open after the fly animation** — the popup opens once `flyTo` finishes so it
   ends up correctly positioned.

## Design

### 1. Header toggle removal

In [`frontend/src/App.tsx`](../frontend/src/App.tsx:302), delete the
`sidebarCollapsed && <button className="collapse-toggle">◀</button>` block from
`topbar-right`. The global summary stays in the header unchanged.

### 2. Sidebar structure (always mounted)

Replace the conditional `!sidebarCollapsed && <aside …>` render with an always
mounted `<aside className="sidebar …">` that:

- keeps the existing header (`h2`, count badge) in the expanded state;
- replaces the `✕` close button with a `<` collapse button;
- renders a slim collapsed edge containing a `>` expand button when collapsed.

The keyboard shortcut `H` ([`useEffect`](../frontend/src/App.tsx:262)) keeps
toggling the same `sidebarCollapsed` state.

### 3. CSS overlay

In [`frontend/src/index.css`](../frontend/src/index.css:103):

- keep `.workspace` as a flex container (so `.map-area` still fills the height)
  and add `position: relative`;
- make `.sidebar` `position: absolute` with `top/bottom/left: 0`, a z-index above
  the map, and a width that transitions between the expanded panel width and the
  slim collapsed edge width;
- add styles for the edge toggle buttons (`>` / `<`) and the collapsed strip.

### 4. Find-on-map popup

Refactor [`focusStation`](../frontend/src/App.tsx:274) to:

1. `map.flyTo([lat, lng], 15, { duration: 0.8 })`;
2. after the fly animation completes (e.g. `map.once('moveend', …)`), open a
   popup at the station coordinates using the default marker icon's popup anchor,
   with content equal to the escaped plain station name (no `<strong>`), matching
   the marker [`Popup`](../frontend/src/App.tsx:392).

## Out of scope

- Backend / BFF / Rust changes.
- Frontend test harness (the frontend has none; build + manual check only).

## Result

Implemented end-to-end in the frontend:

- Header [`topbar`](../frontend/src/App.tsx:302) no longer contains a sidebar
  toggle; only the brand, search trigger, and global summary remain.
- The sidebar is an always-mounted Komoot-style overlay
  ([`sidebar`](../frontend/src/App.tsx:333)) with an in-sidebar `<` collapse
  button and a `>` expand button on the collapsed edge; the map stays full width
  underneath (overlay CSS in [`index.css`](../frontend/src/index.css:103)).
- [`focusStation`](../frontend/src/App.tsx:274) flies to the station (zoom 15),
  then — after the fly animation — opens a popup with the plain station name
  anchored above the marker via the default icon's popup anchor, matching a
  normal map-marker click.

Gates: `make frontend-build` green (`tsc` + `vite build`). Backend gates are
unaffected (no Rust changes).

## Testing / gates

- `make frontend-build` must compile and bundle successfully — green.
- Manual check with `make run` (http://localhost:8081): toggle the sidebar via
  the in-sidebar `<` / `>` buttons, verify the map stays full width, and verify
  "Find on map" opens a popup identical to a marker click.
- Backend gates are unaffected but should stay green: `make check`, `make test`,
  `make test-rest`, `make coverage`.
