---
"@pnpm/config": patch
"pnpm": patch
---

Instead of `pnpm.overrides` replacing `resolutions`, the two are now merged. This is intended to make it easier to migrate from Yarn by allowing one to keep using `resolutions` for Yarn, but adding additional changes just for pnpm using `pnpm.overrides`.
