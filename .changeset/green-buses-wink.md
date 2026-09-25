---
"@pnpm/installing.commands": patch
"pnpm": patch
---

`pnpm add` now suggests initializing the package when run from a workspace-matched directory without a package manifest. Running the command from a directory outside workspace package patterns still prints the root warning [pnpm/pnpm#11345](https://github.com/pnpm/pnpm/issues/11345).
