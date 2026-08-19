---
"pacquet": patch
---

A package's `files` entries now match from the package root, the way npm reads them. A bare `src` also matched nested directories such as `example/src`, so a dependency installed from git shipped the repository's own example app: 13,324 files where the package publishes 45. The same filter decides what `pnpm pack` and `pnpm publish` put in a tarball and what `pnpm deploy` copies, so those stop carrying the extra files too. Exclusions such as `!**/__tests__` and `!*.map` still match at any depth.
