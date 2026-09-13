# 127 - Fix open Sonar issues

Status: implemented

## Problem

The current SonarCloud branch report contains five open findings: one path-traversal warning in an unused screenshot analyzer and four Dockerfile findings caused by combining image tags with digests.

## Approach

1. Remove the unused screenshot analyzer, eliminating the path-injection surface.
2. Use digest-only Docker `FROM` references so each image is identified by one immutable reference.
3. Run focused security and Dockerfile validation, then the repository gates required for the touched files.

## Definition of done

- [x] Unused screenshot analyzer removed
- [x] Backend Dockerfile uses digest-only image references
- [x] Frontend Dockerfile uses digest-only image references
- [x] Focused validation passes
- [x] Required repository gates pass or are documented if unavailable
- [x] Plan status updated to implemented

## Validation

- Focused analyzer and Docker reference checks passed.
- The unused analyzer was removed, eliminating the Sonar path-injection finding.
- `make check` passed: formatting, Clippy, Prettier, and cargo audit.
- `make test-rest` passed: 128 tests.
- `make coverage` passed: backend production coverage 85.69% overall and
	95.41% core coverage; frontend coverage 94.92% lines across 543 tests.
