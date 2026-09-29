---
"pacquet": patch
---

`pnpm install --frozen-lockfile` now fails when `Cargo.lock` does not satisfy a dependency requirement in `Cargo.toml`. The error names the crate and the version the lockfile holds [pnpm/pnpm#16355](https://github.com/pnpm/pnpm/issues/16355).
