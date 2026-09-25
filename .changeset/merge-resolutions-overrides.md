---
"@pnpm/config.reader": patch
"pnpm": patch
"pacquet": patch
---

`resolutions` in the root `package.json` are now promoted to overrides with a deprecation warning when `overrides` are not set. When both exist, `resolutions` are ignored with a warning and `overrides` takes precedence [#10675](https://github.com/pnpm/pnpm/issues/10675).
