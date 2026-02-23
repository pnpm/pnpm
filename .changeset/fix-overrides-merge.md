---
"@pnpm/config": patch
"pnpm": patch
---

Fix overrides from package.json resolutions being lost when pnpm-workspace.yaml has overrides [#10675](https://github.com/pnpm/pnpm/issues/10675)

When both `package.json` has a `resolutions` field and `pnpm-workspace.yaml` has an `overrides` field, the overrides are now properly merged instead of the workspace overrides replacing the package.json ones.
