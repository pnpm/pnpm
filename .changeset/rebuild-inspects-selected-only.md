---
"pacquet": patch
"@pnpm/napi": patch
---

`pnpm rebuild <pkg>` and `pnpm rebuild --pending` no longer read the manifest of every installed package to find build scripts. Only the selected packages are inspected and built. On a large `node_modules` served lazily, such as over a network or FUSE mount, this turned a rebuild of a few packages into a fetch of every package.
