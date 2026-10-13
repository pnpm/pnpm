---
"pacquet": patch
---

`--registry`, `--https-proxy`, `--http-proxy`, and `--no-proxy` now apply when pnpm resolves and downloads the pnpm version a project pins in `packageManager`. Before, these lookups went to `https://registry.npmjs.org/`, which a firewall that only allows a private registry blocks [#16911](https://github.com/pnpm/pnpm/discussions/16911).
