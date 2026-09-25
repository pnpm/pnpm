---
"@pnpm/deps.compliance.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm audit --fix` now prunes redundant overrides when one vulnerable range is a subset of another for the same package [#8577](https://github.com/pnpm/pnpm/issues/8577).
