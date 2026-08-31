---
"pacquet": patch
---

Speed up dependency resolution in large workspaces: workspace `link:` targets are now re-anchored between the lockfile root and each consuming project with lightweight relative-path math instead of rebuilding and comparing absolute paths on every dependency edge [#14352](https://github.com/pnpm/pnpm/issues/14352).
