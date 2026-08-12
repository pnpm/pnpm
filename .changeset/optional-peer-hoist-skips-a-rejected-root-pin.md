---
"@pnpm/installing.deps-resolver": patch
"pacquet": patch
"pnpm": patch
---

An auto-installed *optional* peer is now resolved to a version its declared peer range accepts, even when the workspace root depends on that package at a version outside the range. A root pinning `date-fns@2.30.0` used to hand that version to an importer whose only `date-fns` need was an optional `^4.0.0` peer, which pnpm then reported as an unmet peer [#13867](https://github.com/pnpm/pnpm/issues/13867).
