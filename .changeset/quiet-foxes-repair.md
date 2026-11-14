---
"pnpm": patch
"pacquet": patch
---

`pnpm install --frozen-lockfile` now detects changes to a local child's `injected` setting. Local tarball children using `link:` or a bare path resolve from the lockfile directory [pnpm/tasks#319](https://github.com/pnpm/tasks/issues/319).
