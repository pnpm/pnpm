---
"@pnpm/config.reader": patch
"pnpm": patch
---

pnpm now fails with `ERR_PNPM_INVALID_ALLOW_BUILDS` when `allowBuilds` is not an object or one of its values is not `true`, `false`, or a string. Such values used to be ignored silently.
