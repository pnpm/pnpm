---
"@pnpm/deps.compliance.commands": patch
"@pnpm/deps.compliance.license-scanner": patch
"pnpm": patch
"pacquet": patch
---

`pnpm licenses list` now reports the actual on-disk package locations when using `nodeLinker: hoisted` or `shamefully-hoist: true` [pnpm/pnpm#8589](https://github.com/pnpm/pnpm/issues/8589).
