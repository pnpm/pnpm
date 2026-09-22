---
"pnpm": patch
"pacquet": patch
---

`pnpm install --frozen-lockfile` now rejects changed local tarballs, even when the previous archive contents are in the store [pnpm/pnpm#1889](https://github.com/pnpm/pnpm/issues/1889).
