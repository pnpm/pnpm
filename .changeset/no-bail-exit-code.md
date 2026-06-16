---
"@pnpm/exec.commands": minor
"pnpm": minor
---

`pnpm run --no-bail` now exits with a non-zero exit code when any of the executed scripts fail, while still running every matched script to completion. This makes the exit-code behavior of `--no-bail` consistent between recursive and non-recursive runs (recursive runs already failed at the end). Previously, a non-recursive `pnpm run --no-bail` always exited with code 0, even when a script failed [#8013](https://github.com/pnpm/pnpm/issues/8013).
