---
"@pnpm/symlink-dependency": patch
"@pnpm/headless": patch
"pnpm": patch
---

Reject path-traversal and reserved dependency aliases (such as `../../../escape`, `.bin`, `.pnpm`, or `node_modules`) that come from a lockfile rather than a freshly resolved manifest. A crafted lockfile alias could otherwise be joined directly under a hoisted `node_modules` directory, letting package files be written outside the intended install root or overwrite pnpm-owned layout.

The `nodeLinker: hoisted` graph builder now validates each alias at the directory sink (`safeJoinModulesDir`), matching the validation pnpm already performs when resolving aliases from manifests. See [GHSA-fr4h-3cph-29xv](https://github.com/pnpm/pnpm/security/advisories/GHSA-fr4h-3cph-29xv).
