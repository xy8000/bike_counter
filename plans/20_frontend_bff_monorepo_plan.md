# 20 - Frontend + BFF module + monorepo restructure plan

Status: implemented

## Problem

The repository today is a single Rust crate ([`Cargo.toml`](../Cargo.toml:1))
whose code lives at the repository root, with a REST API ([`rest`](../src/adapter/driving/rest/mod.rs:1))
documented in one OpenAPI document
([`openapi.rs`](../src/adapter/driving/rest/openapi.rs:1)) and browsable via
Swagger UI. Docker Compose ([`docker-compose.yml`](../docker-compose.yml:1))
only starts `db` + `app`, and there is no frontend.

The user wants to add a React frontend and, to keep frontend and backend from
being mixed in code, restructure the repository into a `/frontend` and a
`/backend` folder. The frontend must call a new Backend-for-Frontend (BFF) Rust
module that exposes a dedicated BFF API, and that BFF API must appear in Swagger
under its own collection/tag. `make run` must keep working via Docker Compose.

## Goal

- Restructure into a monorepo: [`frontend/`](../frontend) and [`backend/`](../backend).
- Docker Compose ramps up the whole stack (`db` + `backend` + `frontend`).
- A React (Vite) frontend shows "Hello World" and calls the BFF API as proof of
  integration.
- A BFF Rust **module** inside the existing crate exposes `/api/bff` endpoints on
  the same port `8080`, documented in the **same** Swagger doc under a new tag
  `BFF API`.
- `make run` keeps booting the stack via `docker compose up --build`.

## Decisions (clarified)

1. **Single git repository (monorepo)**, not multiple repositories: two top-level
   folders `frontend/` and `backend/` plus orchestration files at the root.
2. **BFF = module inside the existing `bike_counter` crate** — no new binary, no
   new service, no new port. Endpoints live under `/api/bff`, are served on
   `8080`, and are added to the existing [`ApiDoc`](../src/adapter/driving/rest/openapi.rs:20)
   under a new `BFF API` tag.
3. **Frontend serving**: production Vite build served by **nginx**, which
   reverse-proxies `/api` to the `backend` service, so no CORS setup is needed.
   React fetches `GET /api/bff/hello` and displays the BFF message.

## Design

### 1. Repository restructure

Move the Rust backend into [`backend/`](../backend):

- [`Cargo.toml`](../backend/Cargo.toml), `Cargo.lock`
- [`src/`](../backend/src)
- [`migrations/`](../backend/migrations)
- [`config.toml.example`](../backend/config.toml.example)
- [`Dockerfile`](../backend/Dockerfile)
- [`docker/`](../backend/docker) (entrypoint)

Keep at the repository root (orchestration/docs):

- [`Makefile`](../Makefile:1), [`docker-compose.yml`](../docker-compose.yml:1)
- [`README.md`](../README.md:1), [`ToDo.md`](../ToDo.md:1)
- [`scripts/`](../scripts), [`plans/`](../plans)

### 2. Docker Compose

[`docker-compose.yml`](../docker-compose.yml:1) becomes three services:

- `db` — unchanged (`postgres:16-alpine`, dev defaults, healthcheck).
- `backend` (renamed from `app`) — `build.context: ./backend`,
  `dockerfile: ./backend/Dockerfile`, mounts `./backend/config.toml:/app/config.toml:ro`,
  exposes `8080:8080`, healthcheck unchanged (`/health/ready`).
- `frontend` — `build.context: ./frontend`, exposes `8081:80`, `depends_on:
  backend` (service_started), healthcheck hitting the static page.

The frontend is served at <http://localhost:8081>; the backend stays at
<http://localhost:8080>.

### 3. BFF Rust module

New module [`backend/src/adapter/driving/bff/`](../backend/src/adapter/driving/bff):

- [`mod.rs`](../backend/src/adapter/driving/bff/mod.rs) — re-exports `dto` and `handlers`.
- [`dto.rs`](../backend/src/adapter/driving/bff/dto.rs) — `BffHelloDto { message: String }`
  (`Serialize`, `Deserialize`, `ToSchema`).
- [`handlers.rs`](../backend/src/adapter/driving/bff/handlers.rs) —
  `GET /api/bff/hello` returning `BffHelloDto { message: "Hello from BFF" }`,
  annotated with `#[utoipa::path(..., tag = "BFF API")]` so `__path_get_bff_hello`
  is generated.

Wiring:

- Register `pub mod bff;` in
  [`backend/src/adapter/driving/mod.rs`](../backend/src/adapter/driving/mod.rs:1).
- Add `.route("/api/bff/hello", get(get_bff_hello))` in
  [`create_router`](../backend/src/adapter/driving/rest/mod.rs:69).
- Register `get_bff_hello` in the `paths(...)` list, `BffHelloDto` in the
  `components(schemas(...))` list, and a new
  `(name = "BFF API", description = "Backend-for-Frontend endpoints consumed by the React frontend")`
  tag in [`openapi.rs`](../backend/src/adapter/driving/rest/openapi.rs:20).

`/api/bff` is the reserved namespace for the frontend; the existing `/api/v1`
endpoints remain unchanged.

### 4. React frontend

Create [`frontend/`](../frontend) as a Vite + React + TypeScript app:

- [`package.json`](../frontend/package.json) — `react`, `react-dom`, `vite`,
  `@vitejs/plugin-react`, `typescript`; `build` script `vite build`.
- [`vite.config.ts`](../frontend/vite.config.ts), [`tsconfig.json`](../frontend/tsconfig.json),
  [`index.html`](../frontend/index.html), [`src/main.tsx`](../frontend/src/main.tsx),
  [`src/App.tsx`](../frontend/src/App.tsx), [`src/index.css`](../frontend/src/index.css).
- [`src/App.tsx`](../frontend/src/App.tsx) renders "Hello World" and, on mount,
  fetches `/api/bff/hello`, displaying the returned `message`; on failure it
  falls back to the static "Hello World" text so the page still renders without
  the backend.
- [`Dockerfile`](../frontend/Dockerfile) — multi-stage: `node:20-alpine` build
  (`npm ci && npm run build`) -> `nginx:alpine` runtime.
- [`nginx.conf`](../frontend/nginx.conf) — serves `/usr/share/nginx/html` and
  `location /api/ { proxy_pass http://backend:8080; }`.

### 5. Makefile and scripts

- [`Makefile`](../Makefile:1) `run`/`down` stay `docker compose up --build` /
  `docker compose down`. Cargo targets (`build`, `fmt`, `check`, `test`,
  `test-rest`, `coverage`, `clean`) run inside `backend/` via `cd backend && ...`.
  `logs` follows the whole stack (`docker compose logs -f`).
- [`scripts/fmt-test.sh`](../scripts/fmt-test.sh:14) and
  [`scripts/coverage.sh`](../scripts/coverage.sh:33) `cd` into `backend/` before
  invoking `cargo`.
- [`scripts/docker-compose-test.sh`](../scripts/docker-compose-test.sh:22) points
  `CONFIG_FILE` at `backend/config.toml`, uses the `backend` service name, and
  adds assertions for `GET /api/bff/hello` (200 + message) and the frontend page
  (200).

### 6. Ignore files and docs

- Add `backend/.dockerignore` (target/, config.toml, compose/docs) and
  `frontend/.dockerignore` (node_modules, dist). Trim the root
  [`.dockerignore`](../.dockerignore:1) since builds now use the subfolder
  contexts.
- Update [`.gitignore`](../.gitignore:1) to ignore `frontend/node_modules` and
  `frontend/dist`, and remove the stray `plans` / `example` entries so the new
  plan document is tracked like the existing ones.
- Update [`README.md`](../README.md:1) and [`ToDo.md`](../ToDo.md:1): monorepo
  layout, ports (`8080` backend, `8081` frontend), the BFF API + Swagger
  collection, and frontend run instructions.

## Out of scope

- True multiple-git-repository ("multirepo") split.
- BFF aggregation/transformation of the existing `/api/v1` data (only the
  `hello` endpoint is added now; the module is the seam for future BFF calls).
- CORS handling (not needed — the nginx proxy is same-origin).
- A separate Cargo workspace (the backend remains one crate).

## Testing / gates

- Rust: add a BFF test module under
  [`backend/src/adapter/driving/rest/tests/`](../backend/src/adapter/driving/rest/tests/mod.rs:1)
  asserting `GET /api/bff/hello` returns 200 + message and that the generated
  OpenAPI document contains the `BFF API` tag and the `/api/bff/hello` path.
- Run [`make check`](../Makefile:28), [`make test-rest`](../Makefile:34),
  [`make test`](../Makefile:31), [`make coverage`](../Makefile:42), and
  [`make test-e2e`](../Makefile:37) — all must be green.
- Verify `make run` boots `db` + `backend` + `frontend` and the frontend renders
  "Hello World" (with the BFF message) at <http://localhost:8081>.
