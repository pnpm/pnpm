---
"pnpm": patch
"pacquet": patch
---

Project `engines.node` checks now use the Node.js runtime that pnpm links for the dependency groups being installed. If the lockfile records that runtime, the check uses its resolved version. Production installs ignore a runtime declared only in `devEngines.runtime` [pnpm/tasks#302](https://github.com/pnpm/tasks/issues/302).
