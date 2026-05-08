---
"@pnpm/fs.packlist": patch
"pnpm": patch
---

Fix `pnpm pack` not bundling dependencies listed in `bundleDependencies` (or `bundledDependencies`). The npm-packlist upgrade in pnpm 11 changed its API to require the caller to pre-populate the dependency tree, which the wrapper was not doing — `bundleDependencies` were silently dropped from the tarball [#11519](https://github.com/pnpm/pnpm/issues/11519).
