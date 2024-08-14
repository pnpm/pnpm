---
"pnpm": patch
---

`pnpm setup` should never switch to another version of pnpm.

This fixes installation with the standalone script from a directory that has a `package.json` with the `packageManager` field. pnpm was installing the version of pnpm specified in the `packageManager` field due to this issue.
