---
"pacquet": patch
---

`pnpm install` now generates a Cargo lockfile when a dependency range spans several semver-incompatible versions, such as `>=1, <3`, and the newest of them cannot be resolved. pnpm picks the same versions `cargo` does [pnpm/pnpm#14962](https://github.com/pnpm/pnpm/issues/14962).
