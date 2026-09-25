---
"@pnpm/resolving.local-resolver": patch
"pacquet": patch
"pnpm": patch
---

`injectWorkspacePackages` now hard links a workspace dependency declared with a relative path, such as `workspace:../foo`, the same way it already does for `workspace:*` [#10446](https://github.com/pnpm/pnpm/issues/10446).
