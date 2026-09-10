---
"pacquet": patch
---

pnpm now warns when the root `package.json` declares a non-empty array-form `workspaces` field and the project has no `pnpm-workspace.yaml` [#2255](https://github.com/pnpm/pnpm/issues/2255). Such an install links no project and previously said nothing about why.
