---
"pacquet": patch
---

Commands in a project that pins pnpm and sets `nodeVersion` to a different Node.js major than the one on `PATH` now reuse the pinned pnpm once it is installed. Previously, every command reinstalled it from the registry and failed offline [#16497](https://github.com/pnpm/pnpm/issues/16497).
