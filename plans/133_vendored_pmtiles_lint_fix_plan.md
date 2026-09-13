# 133 - Vendored PMTiles debug bundle lint fixes

Status: implemented

## Problem

Sonar reports reliability/maintainability findings in the vendored, generated ESM
bundle [`frontend/public/debug-libs/pmtiles.js`](../frontend/public/debug-libs/pmtiles.js)
(the transpiled `pmtiles` library served only by the debug map page
`pmtiles-debug-v6.html`; the production app bundles `pmtiles` from npm instead).
The file is third-party generated code, but it is tracked in the repo and has no
in-repo generator, so the findings were fixed in place with behaviour-preserving
edits.

Note: [`sonar-project.properties`](../sonar-project.properties:4) already excludes
`frontend/public/debug-libs/**` from analysis (added in plan 129). The findings
listed here predate that exclusion; the code fixes below remove the underlying
issues so they stay gone even if the exclusion is ever narrowed.

## Findings → fixes

| Line | Finding | Fix |
|---|---|---|
| 334 | Remove this assignment of `lpos` | split the `lpos = pos, lm = null` comma statement into two statements (the value is read at `st.p = lpos` after the `break`, which skips the loop update) |
| 715 | Exporting mutable `var`, use `const` | `var leafletRasterLayer` → `const` |
| 801 | Exporting mutable `var`, use `const` | `var Protocol` → `const` |
| 1029 | Exporting mutable `var`, use `const` | `var Compression` → `const` (+ `(Compression \|\| {})` IIFE argument → `({})`) |
| 1059 | Exporting mutable `var`, use `const` | `var TileType` → `const` (+ `(TileType \|\| {})` → `({})`) |
| 1106 | Exporting mutable `var`, use `const` | `var FileSource` → `const` |
| 1121 | Exporting mutable `var`, use `const` | `var FetchSource` → `const` |
| 1298 | Exporting mutable `var`, use `const` | `var EtagMismatch` → `const` |
| 1334 | Exporting mutable `var`, use `const` | `var ResolvedValueCache` → `const` |
| 1429 | Exporting mutable `var`, use `const` | `var SharedPromiseCache` → `const` |
| 1543 | Exporting mutable `var`, use `const` | `var PMTiles` → `const` |
| 1038 | Add a `yield` statement to this generator | bare `yield;` (the generator implements `defaultDecompress`, which has no `await`, but the `__async` helper *must* drive a generator via `next()`/`throw()`) |
| 1424 | Add a `yield` statement to this generator | bare `yield;` in `ResolvedValueCache.invalidate` for the same reason |
| 996, 999, 1027, 1048, 1056, 1173, 1194, 1200, 1223, 1611 | Use `new Error()` instead of `Error()` | `throw Error(…)` → `throw new Error(…)` |

Behaviour notes:

- The `var` → `const` bindings are never reassigned and are not read before their
  initialization (the two enum IIFEs relied on `var` hoisting for their
  `(X || {})` argument; `({})` is exactly equivalent to `undefined || {}`).
- `yield;` only adds an extra microtask before the (unchanged) result; it does not
  change what the `__async` wrapper resolves/rejects with.
- The `lpos` split is byte-equivalent in effect; the assignment is required
  because a `break` skips the `for` update expression that would otherwise run.

## Verification

- Node dynamic import of the bundle succeeds and exposes every expected export
  (`Compression`, `EtagMismatch`, `FetchSource`, `FileSource`, `PMTiles`,
  `Protocol`, `ResolvedValueCache`, `SharedPromiseCache`, `TileType`,
  `bytesToHeader`, `findTile`, `getUint64`, `leafletRasterLayer`, `readVarint`,
  `tileIdToZxy`, `tileTypeExt`, `zxyToTileId`).
- No `throw Error(` remains; all 10 export bindings are `const`; the two enum
  IIFE arguments are `({})`; both generators contain `yield`.
- `npm run test:unit` — 86 files / 550 tests passed.
- `prettier --check frontend/src` — clean (`public/` is intentionally ignored).
- `make test-playwright` — 74 passed (2.7 min), `e2e-playwright: OK`. This also
  re-validates the plan-132 unprivileged-nginx (port 8080) frontend image and the
  non-root backend image against the real Docker Compose stack.

## Definition of done

- [x] Plan file added
- [x] All listed `var` → `const`, `Error()` → `new Error()`, generator and `lpos` findings fixed
- [x] Bundle imports and exports cleanly under Node
- [x] Frontend unit tests green
- [x] `make test-playwright` green
- [x] Plan `Status:` and boxes updated
