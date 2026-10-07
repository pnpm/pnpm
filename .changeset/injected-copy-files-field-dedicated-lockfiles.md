---
"@pnpm/workspace.injected-deps-syncer": patch
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

With `sharedWorkspaceLockfile: false`, an injected workspace package now holds only the files its `files` field selects, also when the package is installed in the same run. Files such as `tsconfig.json` no longer appear in the injected copy unless `deployAllFiles` is set [#16683](https://github.com/pnpm/pnpm/issues/16683).
