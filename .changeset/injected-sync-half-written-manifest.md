---
"@pnpm/fetching.directory-fetcher": patch
"@pnpm/workspace.injected-deps-syncer": patch
"pnpm": patch
"pacquet": patch
---

A script listed in `syncInjectedDepsAfterScripts` no longer fails when it rewrites `package.json` while pnpm is copying its edits into the injected copies. The sync after the script now replaces a half-copied manifest.
