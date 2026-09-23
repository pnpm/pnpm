---
"@pnpm/exec.commands": patch
"pacquet": patch
"pnpm": patch
---

`pnpm --filter <project> <command>` and `pnpm -r <command>` now run a command installed in the selected projects' dependencies when none of them has a script by that name, as `pnpm <command>` does in a single project. `pnpm run` with `--filter` or `-r` still reports the missing script [#10151](https://github.com/pnpm/pnpm/issues/10151).
