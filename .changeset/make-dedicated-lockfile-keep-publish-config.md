---
"@pnpm/releasing.exportable-manifest": patch
"@pnpm/lockfile.make-dedicated-lockfile": patch
"pnpm": patch
---

`make-dedicated-lockfile` no longer removes fields such as `main` and `types` from the `publishConfig` of the project's `package.json`.
