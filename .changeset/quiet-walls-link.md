---
"@pnpm/deps.graph-hasher": patch
"@pnpm/fs.indexed-pkg-importer": patch
"@pnpm/store.cafs": patch
"@pnpm/store.cafs-types": patch
"@pnpm/store.controller-types": patch
"@pnpm/store.create-cafs-store": patch
"@pnpm/worker": patch
"pacquet": patch
"pnpm": patch
---

The side-effects cache now restores the symlinks that a build script creates inside a package. A warm install used to replace each of them with a copy of its target [#12859](https://github.com/pnpm/pnpm/issues/12859).

After upgrading, every package with a build script is built once more.
