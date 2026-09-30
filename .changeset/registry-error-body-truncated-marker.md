---
"@pnpm/registry-access.commands": patch
"pnpm": patch
---

Registry error messages now always say "(response body truncated)" when pnpm cut the response body short. The marker was missing when the body was cut at exactly 64 KiB.
