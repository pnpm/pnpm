---
"@pnpm/pnpr": patch
---

Fixed pnpr occasionally crashing with a libsql or Turso auth database, most often on Windows. pnpr closed each database connection twice when it dropped one.
