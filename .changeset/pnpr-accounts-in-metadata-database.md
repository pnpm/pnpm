---
"@pnpm/pnpr": patch
---

Accounts and tokens now live in the metadata database, next to package metadata. By default that is `<storage>/.metadata/metadata.sqlite`. The `auth.users.file` and `auth.tokens.file` settings are removed. `pnpr import-htpasswd` writes into the metadata database and no longer takes `--target`.

With the default SQLite database, a password reset made with `pnpr add-user` next to a running server now revokes the account's tokens on the server's next request [zkochan/pnpm-tasks#85](https://github.com/zkochan/pnpm-tasks/issues/85).
