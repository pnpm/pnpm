---
"@pnpm/releasing.versioning": patch
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm change check` now validates the pending change intents in `.changeset/`. It fails when an intent names a package that is not in the workspace or cannot be released.
