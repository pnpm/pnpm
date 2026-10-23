---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
"pacquet": patch
---

With `injectWorkspacePackages: true`, a `workspace:` dependency written as a relative path, such as `workspace:../lib-a`, in a workspace package now resolves from that package's directory [#17284](https://github.com/pnpm/pnpm/issues/17284).
