---
"pacquet": patch
---

`pnpm add` and `pnpm install` now accept `gist:<id>` dependencies and GitHub Gist URLs, and install them from the gist's archive. `pnpm pkg fix` now normalizes a `gist:` repository the way `npm pkg fix` does, and `pnpm licenses` links a gist package to its gist page.
