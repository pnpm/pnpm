---
"pacquet": minor
---

`pnpm install --force` now keeps skipping optional dependencies whose `os`, `cpu` or `libc` do not match the host. It still refetches every package and lifts `engineStrict`. The new `forceIgnoresPlatform` setting restores the previous behaviour, installing optional dependencies of every platform under `--force` [#6133](https://github.com/pnpm/pnpm/issues/6133).
