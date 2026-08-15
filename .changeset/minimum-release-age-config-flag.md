---
"pacquet": patch
---

`--config.minimum-release-age` is no longer ignored. The flag, and its `minimum-release-age-exclude`, `minimum-release-age-ignore-missing-time`, and `minimum-release-age-strict` companions, now override `pnpm-workspace.yaml` the way every other `--config.<key>` token does, so tightening the release-age window for one install works again instead of silently leaving the configured value in place [#13929](https://github.com/pnpm/pnpm/issues/13929).
