---
"pacquet": patch
---

`pnpm install` is faster and uses less memory in large workspaces with `sharedWorkspaceLockfile: false`. Each project's install no longer reads the manifests of every other workspace project again. Installs that wait for their workspace dependencies now count toward `workspaceConcurrency` [#16966](https://github.com/pnpm/pnpm/issues/16966).
