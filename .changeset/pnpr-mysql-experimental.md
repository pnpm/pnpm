---
"@pnpm/pnpr": patch
---

MySQL support for `database.url` is now marked experimental, and pnpr logs a warning at startup when it is configured. SQLite, libSQL, and PostgreSQL stay stable.

pnpr now refuses text that holds a NUL character on every database, so a request that SQLite accepts cannot fail on PostgreSQL.

pnpr instances on different schemas of one PostgreSQL database no longer wait on each other's write and migration locks.
