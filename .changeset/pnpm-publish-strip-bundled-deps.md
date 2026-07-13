---
"pnpm": patch
---

Fix the published `pnpm` package declaring `dependencies` and `devDependencies` that shouldn't be installed by consumers. Because the CLI bundles its runtime dependencies into `dist/node_modules`, the published manifest is now stripped of dependency fields during packing, so `npm install` of the tarball no longer tries to resolve internal-only packages such as `@pnpm/test-ipc-server`. Closes pnpm/pnpm#12955.
