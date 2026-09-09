---
"pacquet": patch
---

pnpm now warns when the root `package.json` declares a `workspaces` field and the project has no `pnpm-workspace.yaml`. The field selected no projects and the install ran as a single project without saying so [#2255](https://github.com/pnpm/pnpm/issues/2255).
