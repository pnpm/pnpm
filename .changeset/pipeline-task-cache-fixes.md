---
"pacquet": patch
---

Fixed several `pnpm pipeline` task caching bugs. `inputs: []` now hashes no input files, `+`-prefixed input globs include gitignored files and workspace files outside the project directory, and `!`-prefixed globs exclude matching files. Tasks that depend on an uncacheable task now re-run after that dependency executes, read-only cached output files restore without permission errors, and task keys hash per-project and custom `lockfileDir` lockfiles.
