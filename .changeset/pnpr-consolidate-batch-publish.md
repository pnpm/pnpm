---
"pacquet": patch
"@pnpm/pnpr": patch
---

`pnpm publish --batch` now sends `PUT /-/pnpr/v0/publish`, and `pnpr` mounts the unified batch publish handler at both `/-/pnpr/v0/publish` and `/npm/-/pnpr/v0/publish`.
