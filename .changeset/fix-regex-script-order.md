---
"@pnpm/exec.commands": patch
"pnpm": patch
"pacquet": patch
---

Regex-matched scripts now preserve `package.json` insertion order when running with `--sequential`, and run in alphabetical order otherwise ([pnpm/pnpm#13174](https://github.com/pnpm/pnpm/issues/13174)).
