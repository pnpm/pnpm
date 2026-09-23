---
"pacquet": patch
"pnpm": patch
---

Running `pnpm install -r` now performs a recursive install across all workspace projects when `recursive-install: false` is configured [pnpm/pnpm#7504](https://github.com/pnpm/pnpm/issues/7504).
