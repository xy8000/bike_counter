# 70 - Frontend Prettier formatting

Status: implemented

## Problem

[`make fmt`](../Makefile:37) only formats the backend via `cargo fmt`. The
frontend has no formatter at all: [`frontend/package.json`](../frontend/package.json:39)
lists neither `eslint`, `tslint` nor `prettier` in `devDependencies`, so the
TypeScript/TSX/CSS/JSON files are formatted inconsistently by hand (and the few
`// eslint-disable-*` comments in `frontend/src/` do nothing because ESLint is
not installed).

## Goal

Add **Prettier** as the single frontend formatter and wire it into `make fmt`
(write) and `make check` (CI check), so the whole monorepo is formatted with one
command. No linting is added (explicit scope decision).

## Decisions

- Prettier only; no ESLint/TSLint (confirmed with the user).
- Config in a new `.prettierrc` matching the existing code style: no semicolons,
  single quotes, trailing commas, `printWidth: 100`, 2-space indent.
- A new `.prettierignore` excludes generated/vendored files: `node_modules/`,
  `dist/`, `playwright-report/`, `test-results/`, `public/` (generated basemap +
  vendored debug-libs), and `package-lock.json`.
- npm scripts: `format` (`prettier --write .`) and `format:check`
  (`prettier --check .`).
- `make fmt` runs backend `cargo fmt` then the frontend `format` script; a
  dedicated `frontend-fmt` target is added. `make check` gains the frontend
  `format:check` gate.

## Changes

### [`frontend/package.json`](../frontend/package.json:1)

- Add `prettier` (current major, e.g. `^3.x`) to `devDependencies`.
- Add the `format` and `format:check` scripts.

### New `frontend/.prettierrc`

```json
{
  "semi": false,
  "singleQuote": true,
  "trailingComma": "all",
  "printWidth": 100
}
```

### New `frontend/.prettierignore`

```
node_modules/
dist/
playwright-report/
test-results/
public/
package-lock.json
```

### [`Makefile`](../Makefile:1)

- `fmt`: append `npm run format --prefix frontend` after `cargo fmt`.
- Add `frontend-fmt` (`npm run format --prefix frontend`) and
  `frontend-fmt-check` (`npm run format:check --prefix frontend`) targets.
- `check`: add `npm run format:check --prefix frontend` after
  `./scripts/fmt-test.sh`.
- Update the `.PHONY` line and the target/help comments.

### [`agents.md`](../agents.md:25)

- Update the gate table row for `make check` to mention the frontend Prettier
  check.

## Verification

- `npm run format:check --prefix frontend` green after the one-time format pass.
- `make fmt` formats both backend and frontend.
- `make check` green.
- `make test-playwright` unaffected (no frontend runtime change), but run once to
  confirm the formatting-only diff does not break anything.

## Gates

- `npm run format:check --prefix frontend` green.
- `make check` green.

## Definition of done

- [ ] Prettier installed + `.prettierrc` + `.prettierignore` added.
- [ ] `make fmt` and `make check` cover the frontend.
- [ ] Formatting-only diff committed.
- [ ] `agents.md` / `README.md` / [`plans/README.md`](../plans/README.md:1)
      updated.
