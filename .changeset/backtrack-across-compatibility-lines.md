---
"pacquet": patch
---

`pnpm install` now falls back to an older semver-incompatible version of a crate when the newest one a dependency range allows cannot be resolved. Ranges such as `>=1, <3` span several of them [pnpm/pnpm#14962](https://github.com/pnpm/pnpm/issues/14962).
