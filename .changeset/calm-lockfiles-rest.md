---
"pacquet": patch
---

In a single-project repository, `pnpm install` and `verifyDepsBeforeRun` now record a successful lockfile check, so later runs skip it until the lockfile changes again. Before, every run repeated the check after the lockfile changed [#16993](https://github.com/pnpm/pnpm/issues/16993).
