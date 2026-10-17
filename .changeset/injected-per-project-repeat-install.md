---
"pacquet": patch
---

With `sharedWorkspaceLockfile: false` and `injectWorkspacePackages: true`, `pnpm install` now updates a project's lockfile and injected copy after a dependency of the injected workspace package changes. Before, the repeat install reported the project as up to date [#17007](https://github.com/pnpm/pnpm/issues/17007).
