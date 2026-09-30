---
"@pnpm/lockfile.fs": patch
"@pnpm/workspace.injected-deps-syncer": patch
"@pnpm/workspace.projects-graph": patch
"pnpm": patch
---

Workspaces now work with a dependency or project named `constructor`, a project directory named `__proto__`, and injected packages that contain files named like `constructor` or `valueOf`. pnpm crashed on some of these names and silently skipped others.
