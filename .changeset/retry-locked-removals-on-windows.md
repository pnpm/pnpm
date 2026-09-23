---
"pacquet": patch
---

On Windows, `pnpm clean` and other commands that remove entries from `node_modules` now wait for up to a minute while another process holds a file there open or runs a program from it, such as an editor extension [pnpm/pnpm#15081](https://github.com/pnpm/pnpm/issues/15081). They used to fail at once with `os error 32` or `os error 5`.
