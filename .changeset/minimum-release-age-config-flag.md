---
"pacquet": patch
---

`--config.minimum-release-age` is honored again, along with `--config.minimum-release-age-exclude`, `--config.minimum-release-age-ignore-missing-time` and `--config.minimum-release-age-strict`. Each overrides the matching `pnpm-workspace.yaml` setting, and the exclude flag may be repeated to build a list [#13929](https://github.com/pnpm/pnpm/issues/13929).
