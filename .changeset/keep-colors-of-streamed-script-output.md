---
"pacquet": minor
---

`pnpm run` and `pnpm exec` now keep the colors of script output when they print it under the project's name, such as with `--stream` or when several scripts run at once. pnpm sets `FORCE_COLOR=1` for these scripts if its own output is in color and `FORCE_COLOR` is not already set.

Script output that redraws a line with `\r`, such as a progress bar, now shows only its last state. Escape codes that move the cursor or clear the screen no longer garble pnpm's output, and long colored lines are cut at the right width.
