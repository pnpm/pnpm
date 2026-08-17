---
"@pnpm/fs.indexed-pkg-importer": patch
"@pnpm/store.controller-types": patch
"pacquet": patch
"pnpm": patch
---

An install sharing a global virtual store no longer removes an incomplete package directory that another importer is still writing, which could fail with `failed to remove existing directory ... prior to swap: Directory not empty`. Such a directory is now repaired in place, and a package file left damaged by an interrupted install is restored instead of being kept.
