---
"pacquet": patch
---

Sped up installs and `pnpm peers check` in workspaces whose projects depend on each other. The peer report now stops at each linked workspace package instead of walking the workspace again for every project that links it. A workspace package's unmet peer dependencies are listed under the projects that link it directly [#14906](https://github.com/pnpm/pnpm/issues/14906).
