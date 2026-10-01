---
"pacquet": patch
---

The `--force` help text of `pnpm install` and `pnpm add` now says that `--force` keeps skipping optional dependencies built for other platforms. It points to `forceIgnoresPlatform` and the `--os`, `--cpu`, and `--libc` options for installing them [#16435](https://github.com/pnpm/pnpm/issues/16435).
