# 72 - Startup migration logging

Status: implemented

## Problem

[`create_pool`](../backend/src/adapter/driven/postgres/pool.rs:37) runs the
refinery migrations with no output
([`migrations::runner().run(&mut client)`](../backend/src/adapter/driven/postgres/pool.rs:48)),
so a first boot (or a migration-heavy upgrade) gives no indication of what is
happening until the server starts.

## Goal

Add concise startup logging around migrations: `Starting DB-Migrations`, then a
short summary of which migrations were applied, or `DB-Migrations: not
necessary` when none ran. Keep it terse — no per-step verbose output.

## Decisions

- Use plain `println!` to match the existing startup logging in
  [`main.rs`](../backend/src/main.rs:126) (no logging framework is currently in
  use).
- Print `Starting DB-Migrations` before running.
- After running, use the refinery `Report.applied_migrations` returned by
  `Runner::run` (or `Runner::get_applied_migrations` before/after if needed) to
  know what was applied this run:
  - empty → `DB-Migrations: not necessary`
  - non-empty → `DB-Migrations: <name-or-version list> ... done`
- The list uses the migration names/versions joined with commas, mirroring the
  requested `1...2...3..` shape without being verbose.

## Changes

### [`backend/src/adapter/driven/postgres/pool.rs`](../backend/src/adapter/driven/postgres/pool.rs:37)

- In `create_pool`, replace the bare `migrations::runner().run(&mut client)`
  with:
  1. `println!("Starting DB-Migrations");`
  2. `let mut runner = migrations::runner();`
  3. `let report = runner.run(&mut client).map_err(...)?;`
  4. print the applied-migration summary based on `report.applied_migrations`.
- Keep the error mapping to `DomainError::Database` unchanged.

## Verification

- `make check` green.
- `make test` green (repository tests spin up a Docker Postgres test container
  and run the migrations through `create_pool`).
- Manual: a fresh boot logs `Starting DB-Migrations` + the migration list +
  `done`; a second boot on an already-migrated DB logs
  `DB-Migrations: not necessary`.

## Gates

- `make check` green.
- `make test` green.

## Definition of done

- [x] Migration logging added and terse.
- [x] `make check` + `make test` green.
