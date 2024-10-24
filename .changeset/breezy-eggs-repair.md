---
"@pnpm/plugin-commands-installation": major
"@pnpm/core": major
"@pnpm/config": major
"pnpm": major
---

The `pnpm link` command adds overrides to the root `package.json`. In a workspace the override is added to the root of the workspace, so it links the dependency to all projects in a workspace.

To link a package globally, just run `pnpm link` from the package's directory. Previously, the command `pnpm link -g` was required to link a package globally.

Related PR: [#8653](https://github.com/pnpm/pnpm/pull/8653).

