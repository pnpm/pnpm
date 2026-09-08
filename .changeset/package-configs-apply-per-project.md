---
"pacquet": patch
---

`packageConfigs` settings now reach the projects they name. In a workspace where each project keeps its own lockfile (`sharedWorkspaceLockfile: false`), an entry's `overrides`, `hoist`, `modulesDir`, `saveExact`, and `savePrefix` apply to that project's install. A workspace that shares one lockfile still ignores these entries, and the install now says which ones it ignored [#14556](https://github.com/pnpm/pnpm/issues/14556).
