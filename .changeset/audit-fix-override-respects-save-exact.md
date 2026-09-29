---
"@pnpm/deps.compliance.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm audit --fix=override` now respects `saveExact` and `savePrefix` when formatting overrides and interactive choices [pnpm/pnpm#13209](https://github.com/pnpm/pnpm/issues/13209).
