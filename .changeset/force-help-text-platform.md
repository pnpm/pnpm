---
"pacquet": patch
---

The `--force` help text of `pnpm install` and `pnpm add` no longer claims that `--force` installs optional dependencies built for other platforms. It now points to `forceIgnoresPlatform` and the `--os`, `--cpu`, and `--libc` options [#16435](https://github.com/pnpm/pnpm/issues/16435).
