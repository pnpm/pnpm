---
"pnpm": patch
---

`pnpm cat-file` can read executable files from the store. `pnpm cache list`, `view`, and `delete` recognize mixed-case package names.

`pnpm clean` removes custom virtual stores with hidden directory names. In Plug'n'Play projects it also removes the generated loader on a full clean.
