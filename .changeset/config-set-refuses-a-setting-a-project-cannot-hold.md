---
"@pnpm/config.commands": minor
"pnpm": minor
---

`pnpm config set` no longer writes a setting to a project's `pnpm-workspace.yaml` that pnpm ignores there. Writing `configDir`, `pnpmHomeDir`, `stateDir` or any of the other settings a project cannot choose now fails with `ERR_PNPM_CONFIG_SET_NOT_A_PROJECT_SETTING`, and the error names the route that does set it — `pnpm config set --global` for those the global config file takes, `--dir` for `dir`, `XDG_CONFIG_HOME` for `configDir`. `pnpm config delete` still clears one a manifest already has [#13629](https://github.com/pnpm/pnpm/issues/13629).
