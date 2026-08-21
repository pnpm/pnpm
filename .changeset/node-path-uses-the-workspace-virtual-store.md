---
"@pnpm/config.reader": patch
"pnpm": patch
---

With `preferSymlinkedExecutables`, `NODE_PATH` again points at the virtual store of the workspace root when pnpm is run from inside a workspace package, so scripts can resolve dependencies that live only in the hoisted store [#13912](https://github.com/pnpm/pnpm/issues/13912).
