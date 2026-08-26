---
"pacquet": patch
---

A package's `files` entries now match only at the package root, the way npm reads them. A bare `src` used to also match nested directories such as `example/src`, so a dependency installed from git could ship the repository's own example app. The same filter decides what `pnpm pack` and `pnpm publish` put in a tarball and what `pnpm deploy` copies, so those stop carrying the extra files too. Exclusions such as `!**/__tests__` and `!*.map` still match at any depth. A package already in the store keeps its old file set until it is fetched again.
