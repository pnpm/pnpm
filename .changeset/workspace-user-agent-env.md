---
"@pnpm/config": patch
"pnpm": patch
---

pnpm no longer expands environment variables in the `userAgent` setting of `pnpm-workspace.yaml`. A `userAgent` value that contains a `${...}` placeholder is ignored.
