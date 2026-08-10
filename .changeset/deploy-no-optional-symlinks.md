---
"@pnpm/releasing.commands": patch
"pacquet": patch
"pnpm": patch
---

Fix `pnpm deploy --no-optional` creating dangling symlinks for transitive optional dependencies.
