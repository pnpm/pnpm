---
"pacquet": patch
---

`pnpm update --interactive` renders its checklist the way pnpm 11 does: group headings and column headers are separators the cursor skips instead of checkboxes that select nothing, the columns of one group line up with the next, `a` toggles all and `i` inverts the selection, and the confirmed selection is echoed as a list of package names [#14423](https://github.com/pnpm/pnpm/issues/14423).
