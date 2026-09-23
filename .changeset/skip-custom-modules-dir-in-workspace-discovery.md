---
"@pnpm/workspace.projects-reader": patch
"@pnpm/workspace.projects-filter": patch
"@pnpm/deps.status": patch
"pnpm": patch
---

pnpm no longer treats packages inside a project's custom `modulesDir` as workspace projects. Before, with a `modulesDir` such as `vendor` and a `packages` pattern such as `**`, a repeat install ran the lifecycle scripts of dependencies that `allowBuilds` had not approved [#15412](https://github.com/pnpm/pnpm/pull/15412).
