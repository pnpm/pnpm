---
"@pnpm/plugin-commands-publishing": patch
"@pnpm/plugin-commands-patching": patch
"@pnpm/package-bins": patch
"@pnpm/fs.find-packages": patch
"@pnpm/cache.api": patch
"pnpm": patch
---

`fast-glob` replace with `tinyglobby` to reduce the size of the pnpm CLI dependencies [#9169](https://github.com/pnpm/pnpm/pull/9169).
