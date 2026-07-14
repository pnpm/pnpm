---
"pacquet": patch
---

Changed `pacquet publish --json` failures to match pnpm's JSON error output: errors are printed as a JSON envelope on stdout, and the human diagnostic is no longer rendered to stderr for this path.
