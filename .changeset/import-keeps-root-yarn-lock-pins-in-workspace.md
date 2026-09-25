---
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm import` in a workspace now keeps the versions pinned by the root `yarn.lock`, `package-lock.json`, or `npm-shrinkwrap.json` when another workspace project's range allows a newer version. Before, the root project got the newest version in its range [#4385](https://github.com/pnpm/pnpm/issues/4385).
