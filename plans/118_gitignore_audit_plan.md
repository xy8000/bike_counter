# 118 - Audit all gitignore files in the repository

Status: implemented

## Context

`git ls-files --others --exclude-standard`-style audit of every ignore file in
the repo, verifying that (a) generated/large/local artifacts are ignored, (b) no
tracked file accidentally matches an ignore rule, and (c) each ignore file's
rules are accurate and non-stale.

## Ignore files inventory (5 total, all tracked, clean working tree)

| File | Purpose |
|---|---|
| [`/.gitignore`](../.gitignore) | Single git source of truth (Rust + VSCode + frontend + tiles) |
| [`/.dockerignore`](../.dockerignore) | Guard for a hypothetical root-level Docker build (none exists) |
| [`/backend/.dockerignore`](../backend/.dockerignore) | Context `./backend` in `docker-compose.yml` |
| [`/frontend/.dockerignore`](../frontend/.dockerignore) | Context `./frontend` in `docker-compose.yml` |
| [`/frontend/.prettierignore`](../frontend/.prettierignore) | Frontend `prettier --check .` exclusions |

Note: there is **no** `frontend/.gitignore` or `backend/.gitignore` — the root
[`/.gitignore`](../.gitignore) covers the whole repo (the `target/` pattern is
unanchored and matches both the root and `backend/` targets).

## Verified-correct behaviours

- No untracked, non-ignored files (`git ls-files --others --exclude-standard`
  is empty) — nothing would be committed by accident.
- No tracked file matches an ignore rule (`git ls-files -ci --exclude-standard`
  is empty) — no "ignored-but-committed" mistakes.
- `git check-ignore -v` confirms coverage of every real artifact present:
  `backend/target/`, root `target/`, `config.toml`, `frontend/node_modules/`,
  `frontend/dist/`, `frontend/playwright-report/`, `frontend/test-results/`,
  `tiles/.pmtiles-bin/` and the 7.3 GB `tiles/map.pmtiles`.
- Intentional tracked files that are *not* ignored: [`/tiles/README.md`](../tiles/README.md)
  (docs for the ignored archive), [`/frontend/.nvmrc`](../frontend/.nvmrc),
  [`/frontend/components.json`](../frontend/components.json),
  [`/frontend/playwright.config.ts`](../frontend/playwright.config.ts),
  [`/.vscode/settings.json`](../.vscode/settings.json) (kept via the
  `!.vscode/settings.json` negation) and [`/.roo/mcp.json`](../.roo/mcp.json)
  (currently an empty `{"mcpServers": {}}` placeholder — no secrets).

## Findings (minor, no functional bug)

1. [`/backend/.dockerignore`](../backend/.dockerignore) lists entries that are
   **outside its `./backend` build context** and therefore never match:
   `docker-compose.yml`, `README.md`, `plans/`, `scripts/`, `Makefile` (lines
   13–18). The only rules that can ever match are `target/`, `debug/`, `.git/`,
   `.gitignore`, `config.toml`. Harmless but misleading.
2. [`/backend/.dockerignore`](../backend/.dockerignore) still references
   `ToDo.md`, which no longer exists in the repo (removed; see
   [`agents.md`](../agents.md)) — stale line.
3. [`/.gitignore`](../.gitignore) has no OS-noise entries (`.DS_Store`,
   `Thumbs.db`, `*.swp`) and does not ignore a local `.roo/` scratch state;
   [`/.roo/mcp.json`](../.roo/mcp.json) is tracked only because it is an empty
   placeholder today. Optional hardening, not a defect.
4. `config.toml` in [`/.gitignore`](../.gitignore) is unanchored, so it also
   ignores any future nested `config.toml`; current layout (single root
   `config.toml` + committed `config.toml.example`) makes this fine.

## Applied changes (approved)

1. [`/backend/.dockerignore`](../backend/.dockerignore): removed the
   out-of-context repo-root entries (`docker-compose.yml`, `README.md`,
   `plans/`, `scripts/`, `Makefile`) and the stale `ToDo.md` line — none of them
   can ever match inside the `./backend` build context. Kept `target/`,
   `debug/`, `.git/`, `.gitignore`, `config.toml`.
2. [`/.gitignore`](../.gitignore): added an `### Operating System ###` block
   (`.DS_Store`, `Thumbs.db`, `*.swp`) and an `### Roo ###` block (`.roo/`).
3. `git rm --cached .roo/mcp.json` — the tracked file was an empty
   `{"mcpServers": {}}` placeholder; it stays on disk but is no longer tracked,
   so the new `.roo/` rule is effective. Local MCP config can now live in
   `.roo/` without being committed.

Re-verified after the changes: `git check-ignore` confirms `.roo/mcp.json`,
`.roo/scratch.txt`, `.DS_Store`, `Thumbs.db`, `*.swp` are now ignored, while
intentional tracked files (`.vscode/settings.json`, `tiles/README.md`,
`frontend/.nvmrc`, `config.toml.example`) remain untracked-exempt. Docs-only
change — no Rust/TypeScript touched, so the code gates do not apply.

## Definition of done

- [x] All ignore files located and read (5 files).
- [x] Ignore rules verified against live artifacts (`check-ignore`, untracked and
      tracked-vs-ignored checks).
- [x] Working tree confirmed to contain only intended changes; no unintended
      files tracked or untracked.
- [x] Plan file documents the findings (this file).
- [x] Cosmetic cleanups applied to [`/backend/.dockerignore`](../backend/.dockerignore)
      and [`/.gitignore`](../.gitignore); `.roo/mcp.json` untracked; re-verified
      with `git status --porcelain` / `git check-ignore`.
