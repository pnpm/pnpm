---
"pacquet": patch
"pnpm": patch
"@pnpm/exec.commands": patch
"@pnpm/workspace.injected-deps-syncer": patch
---

Scripts listed in `syncInjectedDepsAfterScripts` now update injected dependencies while they run. A watcher on the injected package, such as a dev server, sees each change before the script exits [pnpm/pnpm#4410](https://github.com/pnpm/pnpm/issues/4410).
