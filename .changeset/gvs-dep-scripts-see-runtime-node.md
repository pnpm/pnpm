---
"pacquet": patch
---

With `enableGlobalVirtualStore`, dependency build scripts now find the Node.js installed by `devEngines.runtime`. A `postinstall` script that runs `node` no longer fails with "command not found" on machines without a system Node.js [#15652](https://github.com/pnpm/pnpm/issues/15652).
