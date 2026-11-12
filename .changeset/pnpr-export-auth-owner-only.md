---
"@pnpm/pnpr": patch
---

`pnpr export --include-auth` now leaves `users.json` and `tokens.db` readable only by their owner, also when it replaces files from an earlier export [#18008](https://github.com/pnpm/pnpm/pull/18008).
