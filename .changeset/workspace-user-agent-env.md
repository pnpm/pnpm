---
"@pnpm/config.reader": patch
"pnpm": patch
"pacquet": patch
---

pnpm no longer expands environment variables in a `userAgent` set in a project's `pnpm-workspace.yaml`. A `userAgent` with a placeholder in that file is now ignored. Before this fix, pnpm sent the variable's value to the configured registry [#15415](https://github.com/pnpm/pnpm/issues/15415).
