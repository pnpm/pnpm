---
"pacquet": patch
---

Running `node` or `pnpm install` in a project that pins a Node.js version pnpm has not downloaded yet no longer hangs when a `globalPnpmfile` is configured. The pnpmfile is not loaded while pnpm downloads the pinned runtime [pnpm/pnpm#17530](https://github.com/pnpm/pnpm/issues/17530).
