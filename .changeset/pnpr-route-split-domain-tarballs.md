---
"@pnpm/pnpr": patch
---

`POST /-/pnpr/v0/resolve` and `POST /-/pnpr/v0/verify-lockfile` now route tarballs from split-domain private upstream registries through their `/~<name>/` registry endpoints across resolved lockfiles and frozen prefetch frames. Previously, packuments that placed `dist.tarball` on a separate CDN host left raw CDN URLs and presigned query parameters in the returned lockfile.
