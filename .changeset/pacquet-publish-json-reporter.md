---
"pacquet": patch
---

Fixed `pacquet publish --json --reporter=ndjson` and other explicit reporter selections so `--json` keeps stdout as a single machine-readable JSON value, matching pnpm.
