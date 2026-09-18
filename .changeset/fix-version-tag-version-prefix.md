---
"pacquet": patch
---

`pnpm version` now reads `tagVersionPrefix` from `pnpm-workspace.yaml` and the global config file. An empty value removes the `v` prefix. `PNPM_CONFIG_TAG_VERSION_PREFIX` sets the same value from the environment. The `--tag-version-prefix` flag still overrides the configured value [#15044](https://github.com/pnpm/pnpm/issues/15044).
