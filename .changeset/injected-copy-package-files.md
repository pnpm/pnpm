---
"@pnpm/workspace.injected-deps-syncer": patch
"@pnpm/fetching.directory-fetcher": patch
"@pnpm/exec.commands": patch
"@pnpm/exec.lifecycle": patch
"@pnpm/building.after-install": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
"pacquet": patch
---

An injected workspace package now keeps only the files its manifest lists after a lifecycle script or a script named in `syncInjectedDepsAfterScripts` rebuilds it. Before, these refreshes copied every file of the package directory into the injected copy, including files that `files` or `.npmignore` leave out. With `deployAllFiles: true` the copy still gets every file.
