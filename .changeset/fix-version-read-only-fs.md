---
"pnpm": patch
"pacquet": patch
---

`pnpm --version` now reports why the pnpm version a project pins cannot be installed or recorded, then prints the version of the running CLI. It used to fail, which made the command unusable where the filesystem is read-only. `pnpm --version` also honors `--store-dir` and its `--store` alias now [#14831](https://github.com/pnpm/pnpm/issues/14831).
