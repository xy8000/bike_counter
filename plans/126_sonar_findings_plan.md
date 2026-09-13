# 126 - Sonar findings cleanup

Status: implemented

## Problem

Several repo findings flagged by Sonar are straightforward security and maintainability issues in active project files: GitHub Actions are only pinned to major tags, a transient script trusts CLI input too broadly, a frontend sort is relying on default coercion, shell scripts use the less-safe `[` test syntax, and Docker install commands allow lifecycle scripts.

## Approach

1. Pin workflow actions and Docker base images to immutable SHAs/digests where the project is actively maintained.
2. Harden the screenshot-analysis CLI so it rejects traversal attempts and other invalid input.
3. Replace unsafe default sort behavior and shell conditional syntax with the safer equivalents already used elsewhere in the repo.
4. Re-run the relevant checks (frontend unit tests plus targeted shell validation) and leave the plan updated with the resulting status.

## Definition of done

- [x] Workflow actions pinned to full commit SHAs
- [x] Dockerfile install steps hardened against lifecycle-script execution
- [x] CLI path validation added for screenshot analyzer
- [x] Numeric sort compare function added to the monthly chart
- [x] Bash conditionals updated to `[[ ... ]]`
- [x] Relevant validation commands run and pass
- [x] Plan status updated to implemented
