---
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm link` now fails without changing `package.json` or `pnpm-workspace.yaml` when the linked package's name is not a valid npm package name. It used to write that name to both files, which made every later install fail [#17475](https://github.com/pnpm/pnpm/issues/17475).
