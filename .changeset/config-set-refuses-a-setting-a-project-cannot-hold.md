---
"@pnpm/config.commands": minor
"pacquet": minor
"pnpm": minor
---

`pnpm config set` no longer writes a setting to a project's `pnpm-workspace.yaml` that pnpm ignores there. Writing `configDir`, `pnpmHomeDir`, `stateDir` or any of the other machine-level settings to a project manifest now fails with `ERR_PNPM_CONFIG_SET_SKIPPED_PROJECT_KEY` and names where the setting belongs instead — the global config file for those that have a home there, and nothing at all for those pnpm determines itself. `pnpm config delete` still clears one a manifest already has [#13629](https://github.com/pnpm/pnpm/issues/13629).
