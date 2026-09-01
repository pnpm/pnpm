---
"pacquet": patch
---

Fixed `pnpm with current <command>` when global options precede it, such as `pnpm --workspace-root with current --version` [pnpm/pnpm#14413](https://github.com/pnpm/pnpm/issues/14413).

A short-option cluster that mixes a global flag with an option owned by the command, such as `pnpm -ro dist pack-app`, is now parsed like the same options written after the command.
