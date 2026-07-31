---
"@pnpm/resolving.git-resolver": patch
"pnpm": patch
---

The `GIT_TERMINAL_PROMPT=0` guard on `git ls-remote` now reaches the spawned git process: git is run directly instead of through graceful-git, which forwards only `cwd` and dropped the environment override, so resolving a private git repository could still block on an interactive credential prompt [#13421](https://github.com/pnpm/pnpm/issues/13421).
