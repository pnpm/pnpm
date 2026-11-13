---
"@pnpm/pnpr": minor
---

Moved two pnpr configuration keys under `storage`:

- `s3` is now `storage.s3`.
- `database` is now `storage.database`.

`storage` takes either the storage directory or a table. In the table, `storage.path` sets the directory. `--storage` overrides `storage.path`.
