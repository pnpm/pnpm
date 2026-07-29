---
"pacquet": patch
---

Added the `--workspace-root` (`-w`) flag, which runs the command on the root workspace project. `pnpm add -D typescript prettier -w` from a workspace subdirectory now saves to the root `package.json` instead of failing with "unexpected argument '-w' found" [#13031](https://github.com/pnpm/pnpm/issues/13031). Combined with `--recursive`, the flag narrows the run to the root project alone. `-w` may not be used together with `--global`, and may only be used inside a workspace.
