---
"pacquet": patch
---

On Windows, `pnpm clean` and installs that remove or replace a package in `node_modules` failed at once with `os error 32` or `os error 5` while an editor extension held a file of that package open or ran a program from it. pnpm now waits up to a minute for an open file and up to 5 seconds for a running program [pnpm/pnpm#15081](https://github.com/pnpm/pnpm/issues/15081).
