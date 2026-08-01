---
"@pnpm/workspace.projects-graph": patch
"pnpm": patch
---

Workspace dependencies declared with a relative path (e.g. `"foo": "workspace:../foo"`) are no longer silently dropped from the workspace projects graph, so `--filter` selection and the topological order of recursive commands take them into account.
