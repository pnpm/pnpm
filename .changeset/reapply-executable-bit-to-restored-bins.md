---
"@pnpm/bins.linker": patch
"pnpm": patch
---

When a failed install re-copies a bin script from the store, rerunning `pnpm install` now reapplies the executable bit to the bin instead of leaving it non-executable [#12742](https://github.com/pnpm/pnpm/issues/12742).
