---
"@pnpm/bins.linker": patch
"pnpm": patch
---

On Windows, `pnpm env use -g` and `pnpm add -g node@runtime:<version>` now replace a `node.exe` in the global bin directory that is a broken symlink. Previously they failed with `ENOENT` [#5411](https://github.com/pnpm/pnpm/issues/5411).
