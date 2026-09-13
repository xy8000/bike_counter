# 132 - Sonar security findings fix

Status: implemented

Fix the open SonarCloud security findings (branch `fix/sonar-security-findings`).
All six findings are addressed in this plan; none of them changes application
behaviour for legitimate traffic.

## Findings

| # | File | Finding | Severity | Fix |
|---|---|---|---|---|
| 1 | [`frontend/src/features/stations/api.ts`](../frontend/src/features/stations/api.ts) | Client-Side Request Forgery via unsanitized user input (L8) | High | same-origin BFF allowlist guard before every `fetch` |
| 2 | [`frontend/src/features/stations/api.ts`](../frontend/src/features/stations/api.ts) | Server-Side Request Forgery via unsanitized user input (L8) | Medium | same as #1 |
| 3 | [`backend/Dockerfile`](../backend/Dockerfile) | Dependencies resolved without locking versions (L18) | Medium | `cargo build --release --locked` |
| 4 | [`backend/Dockerfile`](../backend/Dockerfile) | Dependencies resolved without locking versions (L30) | Medium | `cargo build --release --locked` |
| 5 | [`.github/workflows/release.yml`](../.github/workflows/release.yml) | `contents: write` at workflow level (L33) | Low | moved to the job that needs it |
| 6 | [`.github/workflows/release.yml`](../.github/workflows/release.yml) | `id-token: write` at workflow level (L34) | Low | moved to the job that needs it |
| 7 | [`backend/Dockerfile`](../backend/Dockerfile) | `alpine` runtime runs as `root` (L33) | Low | dedicated non-root `app` user (`USER app`) |
| 8 | [`frontend/Dockerfile`](../frontend/Dockerfile) | `nginx` runtime runs as `root` (L16) | Low | run the unprivileged `nginx` user on port 8080 |
| 9 | [`frontend/public/pmtiles-debug.html`](../frontend/public/pmtiles-debug.html) | `<script>` missing integrity (L5) | Low | pin version + `integrity` + `crossorigin` |
| 10 | [`frontend/public/pmtiles-debug.html`](../frontend/public/pmtiles-debug.html) | `<link>` missing integrity (L6) | Low | pin version + `integrity` + `crossorigin` |
| 11 | [`frontend/public/pmtiles-debug.html`](../frontend/public/pmtiles-debug.html) | `<script>` missing integrity (L7) | Low | pin version + `integrity` + `crossorigin` |

## 1. Frontend — sanitize BFF request URLs (findings 1 & 2)

[`getJson()`](../frontend/src/features/stations/api.ts:8) fed its `url` argument
straight into `fetch`. When the URL comes from a server-provided HATEOAS link
([`fetchSidebarStats()`](../frontend/src/features/stations/api.ts:34) is handed
`_links.stats.href`), a compromised/spoofed response could point the browser at an
arbitrary third-party origin (SSRF/CSRF gadget).

- New [`frontend/src/lib/apiUrl.ts`](../frontend/src/lib/apiUrl.ts):
  `assertSafeBffUrl(url)` returns the URL only when it is a root-relative path
  under the `/api/bff/` prefix (the only origin the frontend ever talks to). It
  rejects absolute URLs (`https://evil`), protocol-relative URLs (`//evil`),
  backslash-smuggled paths (`/\evil`, `/api/bff/\..`) and control characters —
  and throws otherwise.
- [`frontend/src/features/stations/api.ts`](../frontend/src/features/stations/api.ts:9):
  every `getJson` URL passes through the guard before `fetch`.
- Tests: new [`frontend/src/lib/apiUrl.test.ts`](../frontend/src/lib/apiUrl.test.ts)
  (6 cases) plus a regression case in
  [`frontend/src/features/stations/api.test.ts`](../frontend/src/features/stations/api.test.ts)
  proving `fetchSidebarStats('https://evil.example/steal')` rejects without
  calling `fetch`.

Scope note: the other `features/*/api.ts` modules share the same `getJson` shape
but are not part of these findings; their test fixtures intentionally use
non-BFF link paths (`/api/overview/...`), so they are left for a follow-up to
keep this fix focused and its test churn minimal.

## 2. Backend image — lock resolved versions (findings 3 & 4)

`Cargo.toml`/`Cargo.lock` are copied before the stub build, so both
`cargo build --release` invocations (stub dependency cache + real build) now use
`--locked`, which fails the build instead of silently updating `Cargo.lock`.

## 3. Backend image — non-root runtime (finding 7)

The runtime stage creates a dedicated `app` user (uid/gid 1000, matching the
Docker Compose bind mounts) and switches to it with `USER app`. The app listens
on 8080 (>1024) and only reads `/app/config.toml`; the basemap is written to the
`/data` bind mount, which the host owns as uid 1000, so the unprivileged user is
the intended writer.

## 4. Frontend image — unprivileged nginx (finding 8)

The official `nginx` image starts as root. The runtime stage now:

- chowns the entrypoint template output dir (`/etc/nginx/conf.d`) and the temp
  dir (`/var/cache/nginx`) to `nginx`, and pre-creates `/run/nginx.pid` owned by
  `nginx` (the image's `nginx.conf` uses `/run/nginx.pid`; `/run` itself is not
  writable by a non-root user),
- declares `USER nginx`,
- listens on 8080 (non-root cannot bind <1024).

Correspondingly [`frontend/nginx.conf.template`](../frontend/nginx.conf.template)
listens on 8080, [`frontend/Dockerfile`](../frontend/Dockerfile) `EXPOSE 8080`,
and [`docker-compose.yml`](../docker-compose.yml) maps `8081:8080` with its
healthcheck probing port 8080. The published host port (8081) is unchanged.

## 5. Release workflow — least-privilege permissions (findings 5 & 6)

Removed the workflow-level `permissions` block; each job now declares only what
it needs:

- `build-and-push`: `contents: read` (checkout) + `id-token: write` (cosign keyless signing),
- `release`: `contents: write` (create the GitHub Release).

## 6. PMTiles debug page — subresource integrity (findings 9–11)

Pinned `pmtiles` from the floating `@3` to `3.2.1` (matching the page's existing
major) and added `integrity` (sha384) + `crossorigin="anonymous"` to all three
unpkg subresources. The page sits in `public/` (not covered by tests) and is a
manual debugging aid.

## Verification

- `npx prettier --check .` — clean.
- `npx tsc --noEmit` — clean.
- `npm run test:unit:coverage` — green (thresholds met; new `apiUrl.ts` at 100 %).
- `./scripts/fmt-test.sh` (cargo fmt + clippy `-D warnings`) — `fmt-test: OK`.
- `make test-rest` — 128 passed, 0 failed.
- `cargo metadata --locked` — `LOCK_OK` (lockfile is complete/consistent).
- `python3` YAML parse of [`release.yml`](../.github/workflows/release.yml) —
  no workflow-level `permissions`; `build-and-push` → `contents: read` +
  `id-token: write`; `release` → `contents: write`.
- `docker build --check -f backend/Dockerfile backend/` — "no warnings found".
- Backend user creation replicated against the pinned `alpine` digest —
  `uid=1000(app)`.
- Frontend image built and run: `docker exec id` → `uid=101(nginx)`, host
  `GET /` → `200`, in-container `wget http://127.0.0.1:8080/` → OK.
- `make test-playwright` green (74 passed) — exercises the unprivileged frontend
  image (port 8080) and the non-root backend image end-to-end.

## Definition of done

- [x] Feature branch created (`fix/sonar-security-findings`)
- [x] Plan file added
- [x] `assertSafeBffUrl` guard implemented and used by the stations API
- [x] Guard + stations-API regression tests added
- [x] `cargo build --release --locked` in both backend build steps
- [x] Backend runtime runs as a non-root `app` user
- [x] Frontend runtime runs as non-root `nginx` on port 8080 (compose updated)
- [x] Release workflow permissions moved to job level
- [x] `pmtiles-debug.html` subresources pinned + integrity-protected
- [x] `make check` green (fmt + clippy + prettier; no dependency changes, so cargo audit is unaffected)
- [x] `make test-rest` green (128 passed)
- [x] `make test-playwright` green (74 passed)
- [x] `make coverage` green (frontend unit coverage thresholds met)
- [x] Plan `Status:` and boxes updated
