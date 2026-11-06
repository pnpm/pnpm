---
"pacquet": patch
---

`pnpm install` now checks the `engines`, `os`, `cpu`, and `libc` fields of every workspace project, not only the workspace root. A project's incompatible `engines.pnpm` fails the install. An incompatible `engines.node` prints a warning, or fails the install with `engineStrict` enabled. An unsupported platform prints a warning [pnpm/tasks#281](https://github.com/pnpm/tasks/issues/281).
