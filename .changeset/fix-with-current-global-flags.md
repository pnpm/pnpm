---
"pacquet": patch
---

Fixed `pnpm with current <command>` when global options precede it, such as `pnpm --workspace-root with current --version` [pnpm/pnpm#14413](https://github.com/pnpm/pnpm/issues/14413).

A short-option cluster that mixes a global flag with an option owned by the command, such as `pnpm -ro dist pack-app`, is now parsed like the same options written after the command.

An option written before the command name is now reported as an unknown option unless that command accepts it, instead of being taken for the command to run — `pnpm -P exec echo` and `pnpm -z exec echo` fail the way `pnpm --tag next exec echo` does.
