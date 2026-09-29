---
"@pnpm/config.reader": patch
"pnpm": patch
---

pnpm now reports an `INVALID_SETTING` error when `ignoredOptionalDependencies` or `requiredScripts` in `pnpm-workspace.yaml` is not an array of strings.
