---
"@pnpm/workspace.injected-deps-syncer": patch
"pnpm": patch
"pacquet": patch
---

`syncInjectedDepsAfterScripts` now copies files into injected dependencies when `node_modules` is on another filesystem than the package sources. The sync previously failed with a cross-device link error and made the script run exit with an error [pnpm/pnpm#14703](https://github.com/pnpm/pnpm/issues/14703).
