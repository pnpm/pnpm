---
"@pnpm/config.reader": minor
"pnpm": minor
---

Root `package.json` `resolutions` are promoted to `overrides` with a deprecation warning when no `overrides` are set in `pnpm-workspace.yaml`. When both exist, `resolutions` is ignored with a warning and `overrides` takes precedence.
