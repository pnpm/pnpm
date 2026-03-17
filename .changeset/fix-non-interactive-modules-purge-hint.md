---
"@pnpm/core": patch
"pnpm": patch
---

Improve the non-interactive modules purge error hint to include the `confirmModulesPurge=false` workaround.

When pnpm needs to recreate `node_modules` but no TTY is available, the error now suggests either setting `CI=true` or disabling the purge confirmation prompt via `confirmModulesPurge=false`.

Adds a regression test for the non-TTY flow.
