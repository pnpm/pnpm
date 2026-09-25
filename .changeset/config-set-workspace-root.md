---
"@pnpm/config.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm config set --location=project` and `pnpm config delete --location=project`, run from a package inside a workspace, now write settings that belong in `pnpm-workspace.yaml` to the workspace root's `pnpm-workspace.yaml`. Before, they created a new `pnpm-workspace.yaml` in the current package, which made that package the workspace root. Settings stored in `.npmrc` are still written to the current directory [#13757](https://github.com/pnpm/pnpm/issues/13757).
