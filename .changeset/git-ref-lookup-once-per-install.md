---
"pacquet": patch
---

`pnpm install` now looks up a git dependency's branch or tag once per install, even when many workspace projects depend on it. With `linkWorkspacePackages` enabled, it used to run `git ls-remote` once per project [#16899](https://github.com/pnpm/pnpm/issues/16899).
