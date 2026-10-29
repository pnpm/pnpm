---
"@pnpm/lockfile.utils": patch
"@pnpm/lockfile.verification": patch
"pacquet": patch
"pnpm": patch
---

With `injectWorkspacePackages` and per-project lockfiles, `pnpm install` now updates a project's lockfile when a local tarball that an injected workspace project depends on is replaced. The project's lockfile kept the old version and integrity before.
