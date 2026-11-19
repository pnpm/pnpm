---
"@pnpm/config.reader": patch
"pnpm": patch
"pacquet": patch
---

pnpm uses the `resolutions` field of the root `package.json` as overrides again if `overrides` is not set in `pnpm-workspace.yaml`, and prints a deprecation warning. If both are set, pnpm ignores `resolutions` and prints a warning [#10675](https://github.com/pnpm/pnpm/issues/10675).
