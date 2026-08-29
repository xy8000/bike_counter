# 63 - World backdrop overzoom outside Germany

Status: implemented

## Problem

Outside the Germany OSM basemap extent (`tiles/basemap.pmtiles`, z5-14), the
map rendered a blank beige void at any zoom above 5 (e.g. panning into France,
the Netherlands, or anywhere else on Earth beyond Germany). The coarse world
backdrop (`tiles/world.mbtiles`, Natural Earth land/water, z0-5) has data for
these areas, but the style's `world-water`/`world-land` layers carried an
explicit `"maxzoom": 5`, which hides the layer above that zoom regardless of
whether the underlying vector source keeps serving (overzoomed) z5 tiles.
Germany itself looked fine because the detailed `basemap` source's layers
(`minzoom: 5`) cover it there.

## Approach

Drop the layer-level `maxzoom: 5` from `world-water`/`world-land` in
[`frontend/public/styles/basemap.json`](../frontend/public/styles/basemap.json)
so MapLibre keeps painting the (overzoomed) coarse world backdrop at every zoom
level as a fallback everywhere the detailed Germany basemap has no data. The
source's own `"maxzoom": 5` is unchanged — it still governs which tile z/x/y is
*requested* (MapLibre reuses/overzooms the z5 tile above that), only the
layer's render-visibility cutoff is removed.

## Validation

- Rebuilt and restarted the `frontend` container; confirmed via the browser
  that a location outside Germany (e.g. northern France) now shows land/water
  from the world backdrop instead of a blank void at higher zoom, while a
  Bonn/Germany location still renders full OSM street-level detail unchanged.
- Run the repository gates required by `agents.md`.
