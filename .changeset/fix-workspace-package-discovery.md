---
"@pnpm/config": patch
"pnpm": patch
---

Fix a regression where `pnpm-workspace.yaml` without a `packages` field caused all directories to be treated as workspace projects. This broke projects that use `pnpm-workspace.yaml` only for settings (e.g. `minimumReleaseAge`) without defining workspace packages [#10909](https://github.com/pnpm/pnpm/issues/10909).
