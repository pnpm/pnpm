---
"pacquet": minor
---

`pnpm run` and `pnpm exec` now keep the colors of script output that they print under the project's name, such as with `--stream`. pnpm sets `FORCE_COLOR=1` for these scripts when its own output is in color, unless `FORCE_COLOR` is already set.

Script output is also rendered more cleanly:

- A line that a progress bar redraws with `\r` shows only its last state.
- Escape codes that move the cursor or clear the screen are dropped.
- Long colored lines are cut at the terminal width.
