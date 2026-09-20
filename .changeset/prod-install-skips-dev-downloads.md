---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install --prod` no longer downloads packages that only a devDependency reaches. `--dev` and `--no-optional` do the same for the groups they leave out [#881](https://github.com/pnpm/pnpm/issues/881).
