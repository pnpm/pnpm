---
"pacquet": patch
---

With `enableGlobalVirtualStore`, dependency build scripts now have the workspace root's `node_modules/.bin` on `PATH`, as they do with a local virtual store. A `postinstall` script that runs `node` now finds the Node.js installed by `devEngines.runtime` [#15652](https://github.com/pnpm/pnpm/issues/15652).
