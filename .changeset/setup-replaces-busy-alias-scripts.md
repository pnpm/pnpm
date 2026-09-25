---
"pacquet": patch
---

`pnpm setup` failed with `Text file busy (os error 26)` when `$PNPM_HOME/bin` already held `pn`, `pnpx`, or `pnx` as links to the running pnpm executable. It now replaces those files and completes [#15494](https://github.com/pnpm/pnpm/issues/15494).
