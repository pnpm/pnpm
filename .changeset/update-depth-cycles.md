---
"pacquet": patch
---

`pnpm update --depth <n>` with a large `n`, such as the `--depth=9999` that `update-browserslist-db` runs, now finishes as fast as `--depth Infinity`. It used memory and stack in proportion to `n` on lockfiles with dependency cycles, and could run out of memory or crash [#17329](https://github.com/pnpm/pnpm/issues/17329).
