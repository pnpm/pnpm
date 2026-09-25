---
"pacquet": patch
---

With `enableGlobalVirtualStore`, dependency build scripts now see the workspace root's `node_modules/.bin`, as they do with a local virtual store. A `postinstall` script that runs `node` finds the Node.js installed by `devEngines.runtime` and no longer fails with "command not found" on machines without a system Node.js [#15652](https://github.com/pnpm/pnpm/issues/15652).

Dependency build scripts also see the bins of privately hoisted dependencies.
