---
"pacquet": patch
---

Packages that run a lifecycle script are no longer hard-linked into the virtual store, so a build script can no longer rewrite the workspace source of an injected package or the store copy it was imported from [pnpm/pnpm#15483](https://github.com/pnpm/pnpm/issues/15483).
