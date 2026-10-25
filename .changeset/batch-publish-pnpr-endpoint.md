---
"@pnpm/releasing.commands": patch
"pnpm": patch
---

`pnpm -r publish --batch` now sends `PUT /-/pnpr/v0/publish`, the batch publish endpoint that pnpr serves.
