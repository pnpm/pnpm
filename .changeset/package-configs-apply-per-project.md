---
"pacquet": patch
---

`packageConfigs` now applies. A workspace whose projects each keep their own lockfile (`sharedWorkspaceLockfile: false`) reads the `overrides`, `hoist`, `modulesDir`, `saveExact`, and `savePrefix` a `packageConfigs` entry declares for a project, and applies them to that project's install [#14556](https://github.com/pnpm/pnpm/issues/14556). Both the map form and the `match` list form are read, and a misspelled setting fails the command. In a workspace that shares one lockfile the entries still do nothing, and the install now says which ones it ignored.
