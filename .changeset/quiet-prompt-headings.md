---
"pacquet": patch
---

`pnpm update --interactive` renders its checklist the way pnpm 11 does. Group headings and column headers are separators the cursor skips instead of checkboxes that select nothing. The columns of one group line up with the next. `a` toggles all and `i` inverts the selection. The confirmed selection is echoed as a list of package names [#14423](https://github.com/pnpm/pnpm/issues/14423).
