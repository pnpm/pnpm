---
"pnpm": patch
"pacquet": patch
"@pnpm/cli.utils": patch
"@pnpm/config.package-is-installable": patch
"@pnpm/config.reader": patch
"@pnpm/workspace.projects-filter": patch
"@pnpm/workspace.projects-reader": patch
---

Workspace projects now check `engines.node` against their own Node.js runtime pin.
