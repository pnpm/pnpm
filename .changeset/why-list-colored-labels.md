---
"pacquet": patch
---

`pnpm why` and `pnpm list` no longer print stray `[90m`-style codes in their trees when the terminal supports colors. The bolded labels — the searched package in `pnpm why`, the project header and the matched package in `pnpm list` — dropped the escape byte of the styles they already carried, leaving the color codes as visible text.
