---
"@pnpm/config.reader": patch
"pnpm": patch
---

pnpm now reports an `INVALID_SETTING` error when `configDependencies` in `pnpm-workspace.yaml` is not an object or when an entry does not match the expected specifier string or descriptor object.
