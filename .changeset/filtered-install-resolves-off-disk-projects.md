---
"pacquet": patch
---

In a sparse checkout, a filtered `pnpm install` now uses the dependencies of workspace projects that are not on disk. To resolve peers and optional dependencies, it reads them from the lockfile. The lockfile is then the same as one from a full checkout [#17791](https://github.com/pnpm/pnpm/issues/17791).
