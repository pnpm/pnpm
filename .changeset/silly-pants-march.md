---
"@pnpm/plugin-commands-patching": patch
---

When executing the `patch-commit` command, if `patchedDependencies` does not exist in `package.json`, the configuration will be written to `pnpm-workspace.yaml`.
