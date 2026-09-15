---
"pacquet": patch
---

Sped up installs and `pnpm peers check` in workspaces whose projects depend on each other. A workspace package's unmet peer dependencies are now reported only under the projects that link it directly. Since 12.3.0 a lockfile update in such a workspace took around ten times longer, and `pnpm peers check` could run for many minutes [#14906](https://github.com/pnpm/pnpm/issues/14906).
