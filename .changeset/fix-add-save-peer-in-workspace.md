---
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm add --save-peer` and `pnpm add -P` now save dependencies to `peerDependencies` in workspace packages [pnpm/pnpm#8912](https://github.com/pnpm/pnpm/issues/8912).
