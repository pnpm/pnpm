---
"@pnpm/releasing.versioning": patch
"pnpm": patch
"pacquet": patch
---

`pnpm version -r` no longer writes a versioning-ledger entry with no consumed intents as a bare `intents:` key, which the next run failed to read with `ERR_PNPM_INVALID_VERSIONING_LEDGER`. Empty intent lists are now written as `intents: []`, and the ledger reader accepts the bare form left by earlier releases.
