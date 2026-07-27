---
"pacquet": patch
---

A repeat `pnpm install --frozen-lockfile` is a no-op again when the project has a platform-incompatible optional dependency. The skipped package is kept in `node_modules/.pnpm/lock.yaml` (`.modules.yaml` is what records the skip), so the install can once more recognize an unchanged tree instead of re-running every lifecycle and dependency build script [#13312](https://github.com/pnpm/pnpm/issues/13312).
