# 160 - Vite config native-loader compatibility

Status: implemented

## Goal

Silence the Vite warning from GitHub issue #36 and keep the configs compatible
with Vite's upcoming `configLoader: 'native'` default:

```
JSON import "./package.json" without import attributes (vitest.config.ts:3:17).
```

## Fix

Read `frontend/package.json` in [`vite.config.ts`](frontend/vite.config.ts) and
[`vitest.config.ts`](frontend/vitest.config.ts) through
`readFileSync` + `JSON.parse` instead of a bare `import pkg from './package.json'`
(which native config loading rejects without `with { type: 'json' }` attributes).
Both config files now document this choice with a comment referencing the issue.

## Definition of done

- [x] No JSON import remains in the Vite/Vitest configs
- [x] Both configs document the `readFileSync` rationale
- [x] Prettier check passes
