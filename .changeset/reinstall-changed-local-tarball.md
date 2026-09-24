---
"pacquet": patch
---

`pnpm install` now re-resolves a local tarball dependency whose file was replaced at the same path. It used to report the lockfile as up to date and keep installing the previous version from the store [#2437](https://github.com/pnpm/pnpm/issues/2437).
