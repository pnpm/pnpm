---
"pnpm": patch
"@pnpm/reviewing.dependencies-hierarchy": patch
---

Fix a situation where `pnpm list` and `pnpm why` may not respect the `--depth` argument.
