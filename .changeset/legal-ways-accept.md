---
"@pnpm/npm-resolver": patch
---

Fix `link-workspace-packages=true` incorrectly linking workspace packages when the requested version doesn't match the workspace package's version. Previously, on fresh installs the version constraint is overridden to `*` in the fallback resolution paths, causing any workspace package with a matching name to be linked regardless of version [#10173](https://github.com/pnpm/pnpm/issues/10173).