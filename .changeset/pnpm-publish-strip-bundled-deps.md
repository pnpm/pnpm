---
"pnpm": patch
---

The published `pnpm` package no longer declares `dependencies` or `devDependencies`. Because the CLI bundles its runtime dependencies into `dist/node_modules`, those fields are dropped when packing, so `npm install` of the tarball no longer tries to resolve internal-only packages such as `@pnpm/test-ipc-server`. Closes pnpm/pnpm#12955.
