---
"pacquet": patch
---

The `pnpm` bin of the npm package now works even when the install script that replaces it with the native binary was skipped, as happens under `--ignore-scripts` and under pnpm and Bun, which block build scripts by default [#14346](https://github.com/pnpm/pnpm/issues/14346). Such an install used to fail every command with a shell syntax error. The bin now runs the native binary through Node.js and, in a terminal, says so: reinstalling pnpm with its build scripts allowed makes it start faster.
