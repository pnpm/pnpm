---
"pacquet": patch
---

Honor the `reporter` setting when it is configured in `pnpm-workspace.yaml`, the global configuration, or the `PNPM_CONFIG_REPORTER` environment variable. An explicit `--reporter` still takes precedence, so `reporter: silent` can make silent output the default [#4879](https://github.com/pnpm/pnpm/issues/4879).
