---
"@pnpm/fs.symlink-dependency": patch
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
---

Reject path-traversal and reserved dependency aliases (such as `../../../escape`, `.bin`, `.pnpm`, or `node_modules`) when the `nodeLinker: hoisted` install restores a dependency graph from the lockfile. A crafted lockfile alias could otherwise be joined directly under a hoisted `node_modules` directory, letting package files be written outside the intended install root or overwrite pnpm-owned layout. The hoisted graph now validates each alias at the directory sink, matching the validation pnpm already performs when resolving aliases from manifests.
