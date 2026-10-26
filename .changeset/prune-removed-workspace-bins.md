---
"@pnpm/bins.linker": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-restorer": patch
"pacquet": patch
"pnpm": patch
---

`pnpm install` now removes command shims from `node_modules/.bin` when a workspace dependency renames or removes an entry from its `bin` field [pnpm/tasks#211](https://github.com/pnpm/tasks/issues/211).
