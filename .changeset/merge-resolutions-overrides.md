---
"@pnpm/config.reader": minor
"pnpm": minor
---

Fix handling of `resolutions` in root `package.json` when `overrides` is set in `pnpm-workspace.yaml`. When both exist, pnpm throws `ERR_PNPM_RESOLUTIONS_CONFLICT_WITH_OVERRIDES` by default. Pass `--ignore-resolutions-conflict` to suppress the error and use `overrides` only. When only `resolutions` exists, it is promoted to `overrides` with a deprecation warning, encouraging migration to the `pnpm-workspace.yaml` `overrides` field.
