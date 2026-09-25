---
"@pnpm/releasing.commands": patch
"@pnpm/releasing.exportable-manifest": patch
"pacquet": patch
"pnpm": patch
---

The `publish` command now resolves `workspace:` dependencies from workspace manifests when `node_modules` is not installed. Previously, publishing without `node_modules` failed with `ERR_PNPM_CANNOT_RESOLVE_WORKSPACE_PROTOCOL`. pnpm/pnpm#6567
