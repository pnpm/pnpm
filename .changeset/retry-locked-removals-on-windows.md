---
"pacquet": patch
---

On Windows, `pnpm clean` and other commands that remove entries from `node_modules` now retry while another process, such as an editor extension, briefly holds a file open [pnpm/pnpm#15081](https://github.com/pnpm/pnpm/issues/15081). They used to fail at once with `os error 32`.
