---
"@pnpm/npm-resolver": patch
"pnpm": patch
---

When a project in a workspace has a `publishConfig.directory` set, dependent projects should install the project from that directory [#3901](https://github.com/pnpm/pnpm/issues/3901)
