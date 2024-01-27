---
"@pnpm/config": major
"pnpm": major
---

The default value of the `link-workspace-packages` setting changed from `true` to `false`. This means that by default, dependencies will be linked from workspace packages only when they are specified using the [workspace protocol](https://pnpm.io/workspaces#workspace-protocol-workspace).
