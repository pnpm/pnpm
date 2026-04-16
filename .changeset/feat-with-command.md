---
"@pnpm/cli.parse-cli-args": minor
"@pnpm/config.reader": minor
"@pnpm/engine.pm.commands": minor
"pnpm": minor
---

Add `pnpm with <version|current> <args...>` command. Runs pnpm at a specific version (or the currently active one) for a single invocation, bypassing the project's `packageManager` and `devEngines.packageManager` pins. Uses the same install mechanism as `pnpm self-update`, caching the downloaded pnpm in the global virtual store for reuse.

Examples:

```
pnpm with current install           # ignore the pinned version, use the running pnpm
pnpm with 11.0.0-rc.1 install       # install using pnpm 11.0.0-rc.1
pnpm with next install              # install using the "next" dist-tag
```

Also adds a new `pmOnFail` setting that overrides the `onFail` behavior of `packageManager` and `devEngines.packageManager`. Accepted values: `download`, `error`, `warn`, `ignore`. Can be set via CLI flag, env var, `pnpm-workspace.yaml`, or `.npmrc` — useful when version management is handled by an external tool (asdf, mise, Volta, etc.) and the project wants pnpm itself to skip the check.

```
pnpm install --pm-on-fail=ignore            # direct CLI flag
pnpm_config_pm_on_fail=ignore pnpm install  # env var
# or in pnpm-workspace.yaml:
#   pmOnFail: ignore
```
