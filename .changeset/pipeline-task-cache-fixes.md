---
"pacquet": patch
---

`pnpm pipeline` now hashes no input files when `inputs: []` is configured. `+`-prefixed input globs include gitignored files and workspace files outside the project directory. `!`-prefixed input globs exclude matching files. Tasks that depend on an uncacheable task now bypass cached output restoration and re-run after that dependency executes. Read-only cached output files restore without permission errors. Task keys now hash each project's wanted lockfile across per-project lockfiles, custom `lockfileDir`, and branch lockfiles.
