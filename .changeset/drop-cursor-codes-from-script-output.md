---
"@pnpm/cli.default-reporter": patch
"pnpm": patch
---

Escape codes in script output that move the cursor or clear the screen no longer garble pnpm's output. pnpm keeps only the color codes, and drops those too when its own output is not in color.
