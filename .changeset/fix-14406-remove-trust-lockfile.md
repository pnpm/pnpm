---
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm remove` now accepts `--trust-lockfile` and `--no-trust-lockfile` to control supply-chain policy checks while removing a package [#14406](https://github.com/pnpm/pnpm/issues/14406).
