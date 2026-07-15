---
"pacquet": patch
---

Changed `pacquet publish --json` failures to match pnpm's JSON output: errors are printed as a compact JSON envelope on stdout, explicit reporter output is suppressed, and the human diagnostic is no longer rendered to stderr for this path.
