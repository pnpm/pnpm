---
"pacquet": patch
---

pnpm now switches to the version a project pins in `packageManager` or `devEngines.packageManager` even when `pnpm-workspace.yaml` uses a setting the running pnpm cannot read, such as the `lockfile.includeResolutionSettings` section [#16675](https://github.com/pnpm/pnpm/issues/16675). If pnpm does not switch, it still reports the setting.
