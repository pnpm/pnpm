---
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

OIDC token exchanges now preserve registry URL path prefixes, fixing authentication against registries hosted under paths such as `/npm/`.
