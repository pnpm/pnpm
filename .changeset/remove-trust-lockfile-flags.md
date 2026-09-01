---
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm remove` now accepts `--trust-lockfile`, so a package the supply-chain policy rejects can be removed instead of failing with `Unknown option: 'trust-lockfile'`. The TypeScript CLI also accepts `--trust-policy`, `--trust-policy-exclude` and `--trust-policy-ignore-after` there, matching `pnpm install` and `pnpm add`.
