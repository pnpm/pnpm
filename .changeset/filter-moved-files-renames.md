---
"@pnpm/workspace.projects-filter": patch
"pnpm": patch
"pacquet": patch
---

`--filter "[<since>]"` now selects projects that files were moved out of when git detects the move as a rename [pnpm/pnpm#15481](https://github.com/pnpm/pnpm/issues/15481).
