---
"@pnpm/pnpr": patch
---

`--storage` now moves the default SQLite metadata database along with the storage directory. Before, pnpr kept reading the database under the storage directory of the config file. A database `metadata.url` names anywhere else stays where it is.
