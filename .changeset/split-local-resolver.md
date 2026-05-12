---
"@pnpm/resolving.local-resolver": major
---

Replaced the `resolveFromLocal` export with two narrower exports: `resolveFromLocalScheme` (handles `file:`/`link:`/`workspace:`/`path:`) and `resolveFromLocalPath` (path-shape match by tarball extension or filesystem characters).
