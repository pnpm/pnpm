---
"@pnpm/pnpr": patch
---

On Unix, `pnpr export --include-auth` now writes `users.json` and `tokens.db` with mode 0600, also when it replaces files from an earlier export [#18008](https://github.com/pnpm/pnpm/pull/18008).
