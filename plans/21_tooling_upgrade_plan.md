# 21 - Tooling upgrade: npm/Node + dependency majors + slimmer Docker images + script scoping

Status: implemented

## Problem

The frontend toolchain is drifting and unpinned:

- [`frontend/Dockerfile`](../frontend/Dockerfile:4) builds on the floating
  `node:20-alpine` tag (Node 20 is end-of-life; the bundled npm is an older major)
  and runs on the floating `nginx:alpine` tag.
- [`frontend/package.json`](../frontend/package.json:1) pins old majors — React 18,
  Vite 5, `@vitejs/plugin-react` 4, TypeScript 5.6 — and declares no `engines`
  field; there is no [`.nvmrc`](../frontend/.nvmrc) either, so local and CI
  builds are not reproducible.
- [`backend/Dockerfile`](../backend/Dockerfile:1) builds on `rust:1-slim` and runs
  on `debian:bookworm-slim` plus `ca-certificates` and `curl`; the runtime image is
  larger than necessary for a statically-linkable Rust binary.
- [`docker-compose.yml`](../docker-compose.yml:1) floats `postgres:16-alpine` and
  hard-codes a `curl`-based healthcheck for the backend.

The user wants the latest stable npm and Node toolchain, latest stable dependency
majors, smaller Docker base images (backend especially), and confirmation that the
CI scripts stay backend-only.

## Goal

- Move the frontend to the latest stable npm/Node and latest stable dependency
  majors (React 19, Vite 7, TypeScript 5.9, etc.).
- Slim the backend Docker image (build and runtime) and keep the frontend/db images
  minimal, pinning floating tags where it improves reproducibility.
- Keep `coverage` and `format` gates backend-only, as they already are.

## Decisions (clarified)

1. **Full upgrade**: upgrade the npm CLI to the latest stable release, move to the
   current Node LTS, **and** bump the frontend dependencies to their latest stable
   majors (React 19, Vite 7, `@vitejs/plugin-react` 5, TypeScript 5.9). Confirmed
   with the user.
2. **Backend base image**: build a musl static binary on `rust:1-alpine` and run on
   `alpine` (much smaller than `debian:bookworm-slim`), keeping `curl` (more common
   than busybox `wget`) by installing it via `apk`. This keeps the shell entrypoint
   and the existing `curl`-based healthcheck unchanged while cutting the base from
   ~74 MB to ~5 MB.
3. **Script scoping**: [`scripts/coverage.sh`](../scripts/coverage.sh:33) and
   [`scripts/fmt-test.sh`](../scripts/fmt-test.sh:14) already `cd` into `backend/`
   and are backend-only; keep them that way and do not add a frontend
   coverage/format script.

## Design

### 1. Frontend toolchain and dependency upgrade

In [`frontend/package.json`](../frontend/package.json:1):

- `dependencies`: `react` -> `^19.x`, `react-dom` -> `^19.x`.
- `devDependencies`: `@types/react` -> `^19.x`, `@types/react-dom` -> `^19.x`,
  `@vitejs/plugin-react` -> `^5.x`, `vite` -> `^7.x`, `typescript` -> `^5.9.x`.
  (Exact latest stable versions are resolved at implementation time via
  `npm outdated` / the npm registry.)
- Add an `engines` field, e.g. `"node": ">=20.19 || >=22.12"` (Vite 7's minimum),
  and record the exact npm version via a `packageManager` field if Corepack is
  desired.
- Regenerate [`frontend/package-lock.json`](../frontend/package-lock.json:1) with
  the pinned npm version (`npm install` after the CLI upgrade).
- Add a [`frontend/.nvmrc`](../frontend/.nvmrc) pointing at the chosen Node LTS so
  local shells and future CI pick up the same version.

No source changes are expected for React 19: [`frontend/src/App.tsx`](../frontend/src/App.tsx:1)
uses `useState`/`useEffect` and [`frontend/src/main.tsx`](../frontend/src/main.tsx:1)
uses `createRoot`, both of which are unchanged in React 19. The build is verified
with `make frontend-build` and the end-to-end smoke test below.

### 2. Frontend Dockerfile

In [`frontend/Dockerfile`](../frontend/Dockerfile:1):

- Build stage: `node:20-alpine` -> `node:24-alpine` (current LTS, bundles the
  latest stable npm).
- Runtime stage: pin `nginx:alpine` to a concrete tag (e.g. the current stable
  `nginx:1.29-alpine`) for reproducibility; keep the SPA + `/api` proxy behavior in
  [`frontend/nginx.conf`](../frontend/nginx.conf:1) unchanged.

### 3. Backend Dockerfile (slim build + runtime)

In [`backend/Dockerfile`](../backend/Dockerfile:1):

- Build stage: `rust:1-slim` -> `rust:1-alpine` (musl). Replace the `apt-get`
  block with `apk add --no-cache curl` so `utoipa-swagger-ui` can still download
  its UI assets at build time. Keep the stub-build dependency cache and the
  `find ... -exec touch` recompile trick unchanged.
- Runtime stage: `debian:bookworm-slim` -> `alpine:3.x`. Install
  `ca-certificates` and `curl` via `apk add --no-cache ca-certificates curl` so
  the existing `curl`-based `HEALTHCHECK` keeps working unchanged. Keep
  [`backend/docker/entrypoint.sh`](../backend/docker/entrypoint.sh:1) (Alpine has
  `sh`).
- The binary is statically linked under musl, so no glibc base is required. The
  crate's dependencies are pure Rust (tokio-postgres, refinery, rustls-based ureq,
  flate2's Rust backend), so the musl build is expected to link cleanly; this is
  confirmed by the e2e gate.

### 4. Docker Compose

In [`docker-compose.yml`](../docker-compose.yml:1):

- No backend healthcheck change needed: the runtime keeps `curl`, so the existing
  `curl`-based `healthcheck.test` stays as-is.
- Optionally pin `postgres:16-alpine` to a concrete minor tag.
- No other service changes: `db` is already Alpine, `frontend` is already
  nginx Alpine.

### 5. Script scoping verification

- [`scripts/coverage.sh`](../scripts/coverage.sh:33) already `cd`'s into
  `backend/` and computes coverage only for the Rust crate.
- [`scripts/fmt-test.sh`](../scripts/fmt-test.sh:14) already `cd`'s into
  `backend/` for `cargo fmt`/`clippy`.
- [`scripts/docker-compose-test.sh`](../scripts/docker-compose-test.sh:22) is
  stack-level (no Cargo/npm scope); it continues to pass against the rebuilt
  images, including the new backend healthcheck.
- No change required; only confirm no frontend coverage/format script is added and
  keep the `make` targets backend-only.

## Out of scope

- A Cargo workspace split or any Rust dependency upgrades.
- Adding frontend lint/format/coverage/unit-test tooling.
- `distroless`/`scratch` backend runtime (smallest, but would drop the shell
  entrypoint and the in-container healthcheck command, which the compose
  `depends_on` chain relies on).
- Renovate/Dependabot configuration for ongoing dependency updates.

## Testing / gates

All gates pass on the upgraded stack:

- `make frontend-build` — frontend type-checks and bundles (Vite 7.3.6).
- `make check` — Rust format + clippy gate green.
- `make test` — 221 passed.
- `make test-rest` — 66 passed.
- `make coverage` — backend-only gate green: overall 82.10% (>= 80%), core 96.40%
  (>= 95%).
- `make test-e2e` — boots the rebuilt `db` + `backend` + `frontend` stack: the
  Alpine/musl backend image (with its `curl` healthcheck) builds and serves, the
  React 19 / Vite 7 frontend builds and is served, and all smoke assertions pass.

## Result

- Backend runtime image is now **39.2 MB** (previously a `debian:bookworm-slim`
  based image well over 90 MB).
- Frontend builds on `node:24-alpine` (latest stable npm) and serves from the
  pinned `nginx:1.31-alpine`.
- [`scripts/coverage.sh`](../scripts/coverage.sh:33) and
  [`scripts/fmt-test.sh`](../scripts/fmt-test.sh:14) remain backend-only.
