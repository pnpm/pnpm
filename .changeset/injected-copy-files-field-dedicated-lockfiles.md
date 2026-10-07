---
"@pnpm/workspace.injected-deps-syncer": patch
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

With `sharedWorkspaceLockfile: false`, an injected workspace package installed in the same run as its dependent now holds only the files its `files` field selects. The copy used to also hold other files of the project, such as `tsconfig.json` [#16683](https://github.com/pnpm/pnpm/issues/16683).
