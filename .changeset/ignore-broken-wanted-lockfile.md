---
"pacquet": patch
---

`pnpm install` no longer fails when `pnpm-lock.yaml` exists but cannot be parsed. Matching the TypeScript CLI, the install now prints an "Ignoring broken lockfile" warning, resolves dependencies from the manifests, and rewrites the lockfile. `--frozen-lockfile` still fails on a broken lockfile.
