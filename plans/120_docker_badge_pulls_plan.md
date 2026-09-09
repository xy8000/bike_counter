# 120 - Fix README Docker Hub image badges (docker/v → docker/pulls)

Status: implemented (docs-only change)

## Problem

The two Docker Hub image badges at the top of [`README.md`](../README.md:4)
(`Backend image` and `Frontend image`) render **"invalid response data"**
instead of a version. They use shields.io's `docker/v` badge, which reads
Docker Hub's tags-list API
(`https://hub.docker.com/v2/repositories/{user}/{repo}/tags/`).

The published images are fine — both repositories exist publicly on Docker Hub
with the expected `0.0.1` and `latest` tags (plus the cosign `.sig` signature
tags pushed by the release workflow). Direct queries to Docker Hub's tags API
return clean JSON. The failure is on the shields.io side and is
**deterministic and repo-specific**: the `docker/v` and `docker/image-size`
badge types (both tags-API based) return "invalid response data" for *these*
repositories, while the `docker/pulls` and `docker/stars` badge types (which
read Docker Hub's lightweight repo-summary API instead) render fine (backend
105 pulls, frontend 94 pulls). The `GitHub Release` badge above them works and
already shows the current version (`v0.0.1`).

The most likely trigger is the cosign signature (`.sig`) tag that `cosign
sign` pushes alongside the images (see [`plans/119`](119_release_v0.0.1_plan.md))
— a known cause of this exact shields error for signed images. It is not
something the repository can fix on the image side, so the badges are switched
to a shields Docker badge type that is verified to work for these repos.

## Decision (confirmed with the owner)

Replace the two `docker/v` version badges with `docker/pulls` (pull-count)
badges. The current version stays visible in the adjacent `GitHub Release`
badge, so no information is lost and the badge row stops showing an error.

## Approach

- [`README.md`](../README.md:4) — change the `Backend image` badge URL from
  `img.shields.io/docker/v/xy8000/bike-counter-backend?sort=semver&label=backend`
  to `img.shields.io/docker/pulls/xy8000/bike-counter-backend?label=backend%20pulls`,
  keeping the Docker Hub link target.
- [`README.md`](../README.md:5) — same swap for `xy8000/bike-counter-frontend`
  (`label=frontend%20pulls`).
- Update this plan's `Status:` and tick the Definition-of-done boxes.

## Definition of done

- [x] Both README image badges use `docker/pulls` and still link to Docker Hub
- [x] New badge URLs verified to render real pull counts (backend 105, frontend 94) — not "invalid response data"
- [x] Plan file `Status:` and checkboxes current
- [x] Docs consistent ([`README.md`](../README.md) badge row; no code touched, so `make check` / `make test-rest` are unaffected)
