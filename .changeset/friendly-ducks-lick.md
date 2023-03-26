---
"@pnpm/audit": patch
"pnpm": patch
---

`pnpm audit` should work even if there are no `package.json` file, just a `pnpm-lock.yaml` file.
