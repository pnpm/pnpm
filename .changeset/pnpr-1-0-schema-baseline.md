---
"@pnpm/pnpr": minor
---

pnpr refuses a database created by an alpha release. Drop its pnpr tables together with the hosted store, and pnpr creates them afresh.

On PostgreSQL, pnpr sorts and compares package names, versions, and other keys by bytes, as it does on SQLite and MySQL.
