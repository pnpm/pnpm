---
"@pnpm/workspace.package-patterns": patch
"@pnpm/workspace.projects-reader": patch
"pnpm": patch
---

Wildcards in negated `packages` patterns of `pnpm-workspace.yaml` now match directories whose names start with a dot. For example, `!packages/**` now also excludes `packages/.dev/tool` when another pattern includes `.dev` explicitly.
