---
"@pnpm/pnpr": patch
---

A publish that loses a write race now reports the error as `document_write_conflict` on every registry surface. The message names the package document [#14599](https://github.com/pnpm/pnpm/issues/14599).
