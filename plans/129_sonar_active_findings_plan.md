# 129 - Fix active Sonar findings

Status: implemented

## Problem

SonarCloud reports active shell-test, HTML accessibility, React accessibility, JavaScript/TypeScript convention, and vendored PMTiles findings on this branch.

## Approach

1. Replace remaining single-bracket Bash conditionals with `[[ ... ]]`.
2. Add language and title metadata to debug HTML pages.
3. Fix React keyboard/accessibility handlers and redundant image alt wording.
4. Apply low-risk JavaScript/TypeScript convention fixes and determine the correct treatment for the generated PMTiles bundle.
5. Run focused checks, repository gates, and frontend tests.

## Definition of done

- [x] Active shell, HTML, React, and JS/TS findings addressed
- [x] Vendored/generated PMTiles findings addressed or intentionally excluded with documented rationale
- [x] Focused validation passes
- [x] Required repository gates pass or are documented if unavailable
- [x] Plan status updated to implemented

## Resolution

- Replaced the remaining single-bracket Bash tests with `[[ ... ]]`.
- Added `lang` and `title` metadata to both PMTiles debug pages.
- Converted clickable map marker wrappers to native buttons with accessible labels.
- Removed redundant image wording from alt text and updated affected tests.
- Replaced `NaN` and `String.fromCharCode()` with the preferred equivalents.
- Excluded the vendored `frontend/public/debug-libs/pmtiles.js` bundle from
	Sonar analysis rather than modifying third-party generated code.

## Validation

- `bash -n scripts/e2e-playwright.sh` passed.
- `make check` passed: formatting, Clippy, Prettier, and cargo audit.
- Frontend unit tests passed: 85 files, 543 tests.
- `make test-rest` passed: 128 tests.
- `make coverage` passed: backend production coverage 86.25% overall and
	95.90% core coverage; frontend coverage 94.92% lines.
