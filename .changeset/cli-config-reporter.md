---
"pacquet": patch
---

Honor the `reporter` setting when it is configured in `pnpm-workspace.yaml`, the global configuration, or the `PNPM_CONFIG_REPORTER` environment variable. Configured `reporter: silent` makes silent output the default. An explicit `--reporter` takes precedence [#4879](https://github.com/pnpm/pnpm/issues/4879).
