---
"@pnpm/releasing.commands": patch
"pnpm": patch
---

The package and bump pickers of `pnpm change` now size their page from the terminal height instead of always showing 7 rows. They fall back to 7 rows when the terminal height is unknown [`pnpm/pnpm#13815`](https://github.com/pnpm/pnpm/issues/13815).
