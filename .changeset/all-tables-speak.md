---
"@pnpm/config": major
---

allow loading certificates from `cert`, `ca` and `key` for specific repository
scopes instead of only globally.

These properties are supported in .npmrc, but get ignored by pnpm, this will
make pnpm read and use them as well.
