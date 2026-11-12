---
"@pnpm/pnpr": patch
---

Accounts and tokens now live in the metadata database, so a single-instance registry keeps all of its state in `<storage>/.metadata/metadata.sqlite`. The `auth.users.file` and `auth.tokens.file` settings and the `--include-auth` flag of `pnpr export` are removed. `pnpr import-htpasswd` writes into the metadata database and no longer takes `--target`.

A password reset or token revocation made with `pnpr add-user` next to a running server now takes effect on the server's next request [zkochan/pnpm-tasks#85](https://github.com/zkochan/pnpm-tasks/issues/85).
