---
"@pnpm/exec.npm-lifecycle": patch
"pnpm": patch
---

A lifecycle script run with `unsafePerm: false` now fails with an error when pnpm cannot create `node_modules/.tmp`. It used to hang.
