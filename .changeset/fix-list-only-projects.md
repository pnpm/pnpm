---
"@pnpm/reviewing.dependencies-hierarchy": patch
"@pnpm/reviewing.plugin-commands-listing": patch
"pnpm": patch
---

Fix `pnpm list --only-projects` regression (fixes #10651). When `node_modules` is absent, fall back to the wanted lockfile so workspace projects are still listed. Ensure `--only-projects` is read from CLI options and restrict the dependency graph to workspace importers when the flag is set.
