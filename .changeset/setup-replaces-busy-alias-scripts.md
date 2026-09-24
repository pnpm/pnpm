---
"pacquet": patch
---

`pnpm setup` no longer fails with `Text file busy (os error 26)` when `$PNPM_HOME/bin` already holds `pn`, `pnpx`, or `pnx` as links to the running pnpm executable [#15494](https://github.com/pnpm/pnpm/issues/15494).
