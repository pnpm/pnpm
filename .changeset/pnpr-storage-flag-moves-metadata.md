---
"@pnpm/pnpr": patch
---

`--storage` now moves the default SQLite metadata database along with the storage directory. Before, pnpr kept reading the database under the storage directory of the config file. A database named by `metadata.url` stays where it is.
