---
"pacquet": patch
---

pnpm now switches to the version a project pins in `packageManager` or `devEngines.packageManager` even when `pnpm-workspace.yaml` has a setting the running pnpm cannot read, such as a `lockfile.includeResolutionSettings` section. If pnpm does not switch, it still reports that setting [#16675](https://github.com/pnpm/pnpm/issues/16675).
