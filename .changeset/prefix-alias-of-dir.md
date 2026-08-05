---
"pacquet": patch
---

npm's `--prefix` is accepted as a spelling of `--dir`, and `--store` as a spelling of `--store-dir`, so `pnpm --prefix ../ run test` no longer fails with "unexpected argument '--prefix' found" [#13583](https://github.com/pnpm/pnpm/issues/13583).
