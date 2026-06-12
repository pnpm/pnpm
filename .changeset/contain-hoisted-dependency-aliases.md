---
"@pnpm/fs.symlink-dependency": patch
"@pnpm/installing.deps-resolver": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
---

Reject path-traversal and reserved dependency aliases (such as `../../../escape`, `.bin`, `.pnpm`, or `node_modules`) that come from a lockfile rather than a freshly resolved manifest. A crafted lockfile alias could otherwise be joined directly under a hoisted `node_modules` directory, letting package files be written outside the intended install root or overwrite pnpm-owned layout.

The fix adds two layers:

- The `nodeLinker: hoisted` graph builder now validates each alias at the directory sink (`safeJoinModulesDir`), matching the validation pnpm already performs when resolving aliases from manifests.
- The lockfile verification gate (`verifyLockfileResolutions`) now runs an always-on, policy-independent check that rejects any importer or snapshot dependency alias that is not a valid package name, failing the install early — before any fetch or filesystem work — for every node linker at once.
