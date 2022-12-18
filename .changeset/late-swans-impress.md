---
"pnpm": minor
"@pnpm/core": minor
"@pnpm/headless": minor
---

When the hoisted node linker is used, preserve `node_modules` directories when linking new dependencies. This improves performance, when installing in a project that already has a `node_modules` directory [#5795](https://github.com/pnpm/pnpm/pull/5795).
