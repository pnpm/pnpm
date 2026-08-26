---
"pacquet": patch
---

`pnpm install --dev` and `pnpm deploy --dev` no longer install optional dependencies, and `--prod` now takes precedence when combined with `--dev`, matching the TypeScript pnpm CLI.
