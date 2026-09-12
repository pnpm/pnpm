---
"pacquet": patch
---

pnpm now warns when the root `package.json` declares a non-empty `workspaces` array and the project has no `pnpm-workspace.yaml`. Such an install linked no project and said nothing about why [#2255](https://github.com/pnpm/pnpm/issues/2255).
