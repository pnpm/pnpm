---
"@pnpm/engine.pm.commands": minor
"pnpm": minor
---

`pnpm self-update` now prints progress messages so the command isn't silent: `Checking for updates...` before resolving, `Updating pnpm from vX to vY...` once a newer version is found, and `Successfully updated pnpm to vY` on completion.
