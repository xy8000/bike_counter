# 31 - Search dialog overflow + scrollable list fix

Status: implemented

## Problem

The search dialog
([`frontend/src/features/search/SearchDialog.tsx`](../frontend/src/features/search/SearchDialog.tsx:1))
misbehaves when it opens with many stations:

- The input row (the "search bar") can run out of the screen.
- The dialog background does not adapt: content spills past the panel's
  `bg-background` and `rounded-lg border`.
- The station list is not reliably scrollable; instead the dialog grows and
  overflows the viewport.

## Root cause

[`DialogContent`](../frontend/src/features/search/SearchDialog.tsx:30) is a
`flex max-h-[70vh] flex-col` panel, but:

1. It has no `overflow-hidden`, so when the content is taller than the panel the
   children paint outside the rounded, bordered background.
2. The header row
   ([`SearchDialog.tsx:38`](../frontend/src/features/search/SearchDialog.tsx:38))
   has no `shrink-0`, so it can be squashed when space is tight.
3. The [`ScrollArea`](../frontend/src/features/search/SearchDialog.tsx:66) uses
   `max-h-[60vh] flex-1` without `min-h-0`. In a flex column, a flex item's
   implicit `min-height: auto` stops it shrinking below its content, and the
   `60vh` cap is not coordinated with the dialog's `70vh` cap plus the fixed
   header height. On shorter viewports the sum exceeds `70vh`, the panel clamps,
   and the surplus overflows the panel (input row pushed out of view).

The working sibling [`Sidebar`](../frontend/src/features/sidebar/Sidebar.tsx:60)
already uses the correct pattern: a bounded flex column where the scroll region is
`min-h-0 flex-1`.

## Goal

Make the dialog panel clip to its rounded border and keep the search bar fixed at
the top while the station list scrolls in the remaining space — mirroring the
sidebar pattern. Frontend-only, no behaviour change.

## Fix

In [`frontend/src/features/search/SearchDialog.tsx`](../frontend/src/features/search/SearchDialog.tsx:30):

1. **Clip the panel.** Add `overflow-hidden` to the `DialogContent` `className`
   so the `bg-background` + `rounded-lg border` always contain their children.

   ```tsx
   className="top-[5rem] flex max-h-[70vh] flex-col gap-0 overflow-hidden p-0 translate-y-0 sm:max-w-[560px]"
   ```

2. **Pin the header row.** Add `shrink-0` to the search-bar strip so the input
   and close/clear buttons never collapse:

   ```tsx
   <div className="flex shrink-0 items-center gap-2 border-b p-3">
   ```

3. **Make the list scroll.** Change the `ScrollArea` from `max-h-[60vh] flex-1`
   to `min-h-0 flex-1` so it occupies the remaining height and becomes the actual
   scroll container (single height budget driven by the dialog's `max-h-[70vh]`):

   ```tsx
   <ScrollArea className="min-h-0 flex-1">
   ```

No changes to [`scroll-area.tsx`](../frontend/src/components/ui/scroll-area.tsx:1)
or any backend code.

## Verification

- `cd frontend && npm run build` green (TypeScript strict + Vite production build).
- `make test-playwright` green (existing
  [`frontend/e2e/search.spec.ts`](../frontend/e2e/search.spec.ts:1) still opens the
  dialog and finds a station).
- Manual smoke test: open the search dialog, confirm the input row stays visible,
  the panel background/border fully enclose the content, and the list scrolls when
  there are many results (e.g. empty query showing all stations).

## Definition of done

- [x] `DialogContent` clips (`overflow-hidden`) and the search-bar row is `shrink-0`.
- [x] `ScrollArea` scrolls the list (`min-h-0 flex-1`), input row pinned above it.
- [x] `make frontend-build` (or `cd frontend && npm run build`) green.
- [x] `make test-playwright` green (frontend e2e).
