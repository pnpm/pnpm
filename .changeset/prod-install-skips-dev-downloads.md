---
"@pnpm/deps.graph-builder": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install --prod` no longer downloads the registry packages that only a devDependency reaches [#881](https://github.com/pnpm/pnpm/issues/881).
