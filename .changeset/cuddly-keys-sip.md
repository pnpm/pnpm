---
"pnpm": patch
"@pnpm/store.index": patch
"@pnpm/store.pkg-finder": patch
"@pnpm/worker": patch
---

Improve deploy and install performance by reusing decoded store index entries in read-only package-file lookup paths.
