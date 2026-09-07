---
"@pnpm/store.cafs": patch
"pnpm": patch
"pacquet": patch
---

Fixed concurrent installs sharing a store occasionally failing with an ENOENT error while importing a package file [#14353](https://github.com/pnpm/pnpm/issues/14353).
