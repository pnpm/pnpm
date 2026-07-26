---
"pacquet": patch
---

Executables that a package ships inside its own tarball (`bundledDependencies`) are linked again into that package's `node_modules/.bin`, under both the isolated and the hoisted node linker. A package that declares `bundleDependencies: true` instead of a list of names is now recorded in `pnpm-lock.yaml` the way pnpm 11 records it, and such a lockfile can be read back.
