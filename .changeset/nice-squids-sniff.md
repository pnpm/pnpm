---
"@pnpm/plugin-commands-rebuild": patch
pnpm: patch
---

The `pnpm rebuild` command should not add pkgs included in `ignoredBuiltDependencies` to `ignoredBuilds` in `node_modules/.modules.yaml` [#9338](https://github.com/pnpm/pnpm/issues/9338).
