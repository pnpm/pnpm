---
"pacquet": patch
---

`pnpm outdated` now aligns its table borders when the output is colorized. The color escape codes in the `Package` and `Latest` cells were being counted as visible characters, so the columns and box-drawing borders drifted out of alignment on a terminal.
