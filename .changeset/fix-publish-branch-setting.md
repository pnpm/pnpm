---
"pacquet": patch
---

`pnpm publish` now reads the `publishBranch` setting from `pnpm-workspace.yaml` and from `PNPM_CONFIG_PUBLISH_BRANCH`. The `--publish-branch` flag overrides it.
