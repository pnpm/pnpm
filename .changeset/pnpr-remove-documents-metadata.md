---
"@pnpm/pnpr": minor
---

Removed the `documents` metadata backend. pnpr keeps package metadata only in SQL: the SQLite database in the storage directory by default, or the database `metadata.url` names. A store that kept its metadata as documents starts with empty metadata. A config that sets `metadata.backend` is refused, and an `s3:` store that hosts packages needs a libSQL `metadata.url`. `migrate-storage` moves archives only and needs that libSQL database to hold the metadata already.
