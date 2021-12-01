---
"@pnpm/lockfile-utils": patch
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

Non-standard tarball URL should be correctly calculated when the registry has no traling slash in the configuration file [#4052](https://github.com/pnpm/pnpm/issues/4052). This is a regression caused introduced in v6.23.2 caused by [#4032](https://github.com/pnpm/pnpm/pull/4032).
