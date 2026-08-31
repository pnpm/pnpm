---
"pacquet": patch
---

`pnpm pm <command>` works again: the `pm` prefix, which forces pnpm's built-in command over a `package.json` script of the same name, is recognized instead of failing with `ERR_PNPM_RECURSIVE_EXEC_FIRST_FAIL` / `Command "pm" not found`. `pnpm pm clean` and `pnpm pm purge` now remove `node_modules` even when the project (or the workspace root) declares a `clean` / `purge` script [#14226](https://github.com/pnpm/pnpm/issues/14226).
