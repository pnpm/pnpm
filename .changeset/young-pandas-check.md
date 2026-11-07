---
"pnpm": patch
"pacquet": patch
"@pnpm/config.reader": patch
"@pnpm/installing.commands": patch
"@pnpm/pkg-manifest.utils": patch
"@pnpm/workspace.projects-filter": patch
"@pnpm/workspace.projects-reader": patch
---

A workspace project that downloads its own Node.js through `devEngines.runtime` now has its `engines.node` checked against that Node.js. Before, pnpm checked it against the shared Node.js version, so `engineStrict` could reject a project whose scripts run on a compatible runtime [pnpm/tasks#300](https://github.com/pnpm/tasks/issues/300).
