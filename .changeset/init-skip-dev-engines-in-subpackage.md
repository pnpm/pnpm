---
"@pnpm/workspace.commands": patch
"pnpm": patch
---

`pnpm init` no longer adds the `devEngines.packageManager` field when run inside a workspace subpackage. The field is only added to the workspace root's `package.json`.
