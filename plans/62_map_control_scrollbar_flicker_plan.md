# 62 - Map control scrollbar flicker

Status: implemented

## Problem

While the visible-stations sidebar changes from its initial loading state to
station rows, its scroll area can briefly expand the document beyond the
viewport. The browser then adds a vertical scrollbar. Because MapLibre anchors
the navigation control to the right side of the map container, the resulting
19px viewport-width change makes the zoom control jump left and back.

## Approach

Contain overflow at the full-screen map-page shell. The sidebar keeps its
existing internal scroll area, while temporary child overflow can no longer
change the document viewport or the map's width.

## Validation

- Reload the map and sample the document/client widths while sidebar rows load.
  The map control must stay at a constant horizontal position.
- Run the frontend production build.
- Run the repository gates required by `agents.md`.