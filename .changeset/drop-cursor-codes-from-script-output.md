---
"@pnpm/cli.default-reporter": patch
"pnpm": patch
---

Escape codes in script output that moved the cursor or cleared the screen garbled pnpm's output. pnpm now drops them and keeps only color codes. Color codes are dropped too when pnpm's own output is not in color.
