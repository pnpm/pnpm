---
"pacquet": patch
---

Global commands ignore `PNPM_CONFIG_WORKSPACE_DIR` when loading workspace settings. For one minor release, they warn when the pnpm home's `pnpm-workspace.yaml` supplies settings. Move package settings under `globalPackagesConfig` in `config.yaml` and machine settings to the top level [pnpm/tasks#321](https://github.com/pnpm/tasks/issues/321).
