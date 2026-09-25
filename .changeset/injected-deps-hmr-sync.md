---
"pacquet": patch
"pnpm": patch
"@pnpm/exec.commands": patch
"@pnpm/store.create-cafs-store": patch
"@pnpm/workspace.injected-deps-syncer": patch
---

`syncInjectedDepsAfterScripts` updates injected dependencies while the script is still running. Each update is a file copy, so a watcher on the injected package sees the change during a long-running script such as a dev server. An injected package is hardlinked when nothing will build it, and copied when a lifecycle script will. Later writes to the source show up on the injected path [pnpm/pnpm#4410](https://github.com/pnpm/pnpm/issues/4410).
