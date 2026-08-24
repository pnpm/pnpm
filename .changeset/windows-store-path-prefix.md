---
"pacquet": patch
---

On Windows, `pnpm store path` now returns a conventional drive path without the `\\?\` verbatim prefix when the project and pnpm home are on different drives [#13987](https://github.com/pnpm/pnpm/issues/13987).
