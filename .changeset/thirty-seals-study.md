---
"@pnpm/plugin-commands-installation": patch
"@pnpm/config": minor
"pnpm": patch
---

The `getConfig` function from `@pnpm/config` now reads the `pnpm-workspace.yaml` file and stores `workspacePackagePatterns` in the `Config` object. An internal refactor was made in pnpm to reuse this value instead of re-reading `pnpm-workspace.yaml` multiple times.
