# 117 - Tidy the plan files (Status lines + ticked Definition-of-done boxes)

Status: implemented

## Problem

The [`plans/`](plans) folder holds 116 numbered plan documents, most of them
long-implemented, but many are inconsistent:

- Several files carry stale `Status:` lines (`drafted`, `proposed`, `planned`,
  `in progress`) even though the described feature shipped (verifiable in git
  history / the codebase).
- Files that were completed before the "tick the Definition of done" habit
  started still contain unticked checklists for work that is done.
- Many files reference the repository tracking docs
  [`ToDo.md`](../ToDo.md) and [`plans/README.md`](README.md), which are being
  removed from the repository. Those links are now dead.

## Goal / Decisions

1. The `plans/` folder stays as the single, self-contained history of plans:
   **all 116 files are kept**, each made internally consistent.
2. Every plan file gets a `Status:` line directly under its H1 title that
   reflects the true state: `implemented` (work shipped), `in progress`,
   `superseded`, `dropped`, or `decided` — matching what git history and the
   codebase show, not stale labels.
3. All "Definition of done" / implementation-step checkboxes that describe
   completed work are ticked (`[x]`).
4. Checklist lines that exist only to record updates to the deleted
   [`ToDo.md`](../ToDo.md) / [`plans/README.md`](README.md) tracking docs are
   removed from the affected plan files (the registration/tracking step no
   longer applies).
5. [`agents.md`](../agents.md) is updated so the mandated workflow no longer
   references `ToDo.md` or `plans/README.md`; a plan only needs its numbered
   file in `plans/`.
6. No product code, configuration or behaviour is changed — docs/plan files
   only.

## Out of scope

- [`ToDo.md`](../ToDo.md) and [`plans/README.md`](README.md) stay deleted
  (their removal is the context of this tidy-up; not re-created here).
- No plan is deleted or rewritten beyond the status/checkbox/registry-reference
  tidy described above.

## Validation

- Docs-only change: no Rust/TypeScript touched, so the code gates do not apply.
- `grep`-checks (all green after the tidy):
  - every `plans/*.md` carries a `Status:` marker within its first six lines;
  - no plan has unticked Definition-of-done boxes;
  - no Definition-of-done checklist line references `ToDo.md` /
    `plans/README.md`, and [`agents.md`](../agents.md) no longer references them;
  - historical prose in older plans may still mention the removed tracking docs
    as a record of what was done at the time (not rewritten).
