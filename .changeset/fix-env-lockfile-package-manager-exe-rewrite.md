---
"@pnpm/installing.env-installer": patch
"pacquet": patch
"pnpm": patch
---

pnpm no longer rewrites `packageManagerDependencies` in `pnpm-lock.yaml` when that block pins `@pnpm/exe` beside `pnpm`. The rewrite ran on every command, so `pnpm list` left a clean working tree dirty, and `pnpm version` then refused to run [#14926](https://github.com/pnpm/pnpm/issues/14926).
