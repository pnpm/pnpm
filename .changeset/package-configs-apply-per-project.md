---
"pacquet": patch
---

A `packageConfigs` entry now reaches the project it names. In a workspace where each project keeps its own lockfile (`sharedWorkspaceLockfile: false`), the entry's `overrides`, `hoist`, `modulesDir`, `saveExact`, and `savePrefix` apply to that project's install [#14556](https://github.com/pnpm/pnpm/issues/14556). Previously the whole setting was ignored, with no warning. Both spellings are read, the map from project name to settings and the list of entries with a `match` key. A setting name the entry misspells now fails the command. A workspace that shares one lockfile still ignores these entries, and the install now names the ones it ignored.
