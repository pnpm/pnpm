---
"pacquet": patch
---

pnpm now detects the same CI environments as pnpm 11, including AWS CodeBuild, which does not set `CI`. On these services `pnpm install` uses a frozen lockfile by default and fails with `ERR_PNPM_OUTDATED_LOCKFILE` when the lockfile is outdated.
