---
"@pnpm/config": patch
"pnpm": patch
---

Add a warning when the current directory contains the PATH delimiter character. On macOS, folder names containing forward slashes (/) appear as colons (:) at the Unix layer. Since colons are PATH separators in POSIX systems, this breaks PATH injection for `node_modules/.bin`, causing binaries to not be found when running commands like `pnpm exec`.

Closes [#10457](https://github.com/pnpm/pnpm/issues/10457).
