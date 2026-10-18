---
"@pnpm/workspace.injected-deps-syncer": patch
"@pnpm/fetching.directory-fetcher": patch
"@pnpm/fs.indexed-pkg-importer": patch
"pnpm": patch
---

A script listed in `syncInjectedDepsAfterScripts` now keeps a package's links to its own files as links in the injected copies, the way `pnpm install` writes them. It no longer fails when the package links to one directory twice or links back to a directory above the link. `pnpm install` with `deployAllFiles` no longer fails on such a link back either.
