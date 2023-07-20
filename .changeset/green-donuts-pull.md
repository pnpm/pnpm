---
"@pnpm/store.cafs": patch
"pnpm": patch
---

The length of the temporary file names in the content-addressable store reduced in order to prevent `ENAMETOOLONG` errors from happening [#6842](https://github.com/pnpm/pnpm/issues/6842).
