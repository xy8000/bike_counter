# 60 - Fix synthetic vector-tile layer version

Status: implemented

## Problem

The synthetic basemap generator writes MVT layers with `version: 1`. MapLibre
expects vector-tile spec v2 layer metadata and reports a warning for the
`basemap` source, which can cause rendering differences or errors.

## Change

Set the generated MVT layer version to 2 for both the world and detail fixture
tiles. Keep the tile geometry, source routing, and production PMTiles path
unchanged.

## Verification

- Regenerate the synthetic MBTiles and confirm the generator self-check passes.
- Build the frontend.
- Load the running map and confirm the MapLibre v2 warning is absent.
