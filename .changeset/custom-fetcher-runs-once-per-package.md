---
"pacquet": patch
---

A `fetchers` pnpmfile hook now runs once per package on a fresh install when it handles a resolution with a custom `type` or delegates a git-hosted one to the same subdirectory. These packages were fetched a second time for installation, so the installed files could come from a different archive than the one their dependencies were read from [#15584](https://github.com/pnpm/pnpm/issues/15584).
