---
"@pnpm/pnpr": patch
---

pnpr metadata reads and writes now retry on a fresh connection when `sqld` or Turso expires or drops a libSQL stream mid-transaction [zkochan/pnpm-tasks#44](https://github.com/zkochan/pnpm-tasks/issues/44).
