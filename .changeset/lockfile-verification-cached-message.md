---
"@pnpm/core-loggers": minor
"@pnpm/cli.default-reporter": minor
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

Print a "Lockfile passes supply-chain policies (verified 2h ago)" message when lockfile verification is skipped because a cached verdict for the same lockfile content and policy is reused. Previously the cached short-circuit was completely silent, which made it look like the policy gate never ran [#12324](https://github.com/pnpm/pnpm/issues/12324).
