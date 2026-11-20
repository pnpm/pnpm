---
"pacquet": patch
"@pnpm/pnpr": patch
---

`pnpm install` now includes dependencies referenced by weak Cargo features in `Cargo.lock`. Cargo no longer rejects the generated lockfile with `--locked` for crates such as `uuid` [pnpm/pnpm#14978](https://github.com/pnpm/pnpm/issues/14978).
