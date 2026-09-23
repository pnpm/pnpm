---
"pacquet": patch
---

On Windows, `pnpm clean` and other commands that remove packages from `node_modules` failed at once with `os error 32` or `os error 5` when an editor extension held a file there open or ran a program from it. They now wait up to a minute for an open file and up to 5 seconds for a running program [pnpm/pnpm#15081](https://github.com/pnpm/pnpm/issues/15081).
