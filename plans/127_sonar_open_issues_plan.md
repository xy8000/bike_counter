# 127 - Fix open Sonar issues

Status: in progress

## Problem

The current SonarCloud branch report contains five open findings: one path-traversal warning in the screenshot analyzer and four Dockerfile findings caused by combining image tags with digests.

## Approach

1. Resolve the screenshot path and workspace through the filesystem before checking containment, preventing symlink escapes from the accepted workspace.
2. Use digest-only Docker `FROM` references so each image is identified by one immutable reference.
3. Run focused security and Dockerfile validation, then the repository gates required for the touched files.

## Definition of done

- [ ] Screenshot analyzer rejects traversal and symlink escape paths
- [ ] Backend Dockerfile uses digest-only image references
- [ ] Frontend Dockerfile uses digest-only image references
- [ ] Focused validation passes
- [ ] Required repository gates pass or are documented if unavailable
- [ ] Plan status updated to implemented
