---
"@pnpm/installing.commands": patch
"pnpm": patch
---

`pnpm install` now installs a workspace project located in a parent directory of the workspace root, such as one matched by `packages: ['..']` [#10096](https://github.com/pnpm/pnpm/issues/10096).
