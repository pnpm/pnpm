---
"pacquet": patch
---

`pnpm link <dir>` now resolves a relative `<dir>` against the current directory. In a directory without a `package.json`, such as a workspace's `packages` folder, it used to resolve the path against the nearest project above it. So `pnpm link .` linked that project into itself [#17475](https://github.com/pnpm/pnpm/issues/17475).
