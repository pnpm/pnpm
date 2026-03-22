---
"pnpm": major
---

`pnpm list`, `pnpm ll`, `pnpm la`, and `pnpm why` now run on all workspace projects by default when executed inside a workspace, matching the behavior of `pnpm install` and `pnpm audit`. Use `--no-recursive` to check only the current project.
