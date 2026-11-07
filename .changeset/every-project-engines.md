---
"pacquet": patch
---

`pnpm install`, `pnpm add`, `pnpm update`, and `pnpm remove` now check the `engines`, `os`, `cpu`, and `libc` fields of every workspace project, not only the workspace root. `pnpm run -r` and `pnpm exec -r` check them before running anything. A project's incompatible `engines.pnpm` fails the command. An incompatible `engines.node` prints a warning, or fails the command with `engineStrict` enabled. An unsupported platform prints a warning [pnpm/tasks#281](https://github.com/pnpm/tasks/issues/281) [pnpm/tasks#299](https://github.com/pnpm/tasks/issues/299).
