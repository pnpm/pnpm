---
"@pnpm/plugin-commands-deploy": major
"pnpm": minor
---

A new experimental command added: `pnpm deploy`. The deploy command takes copies a project from a workspace and installs all of its production dependencies (even if some of those dependencies are other projects from the workspace).

For example, the new command will deploy the project named `foo` to the `dist` directory in the root of the workspace:

```
pnpm --filter=foo deploy dist
```
