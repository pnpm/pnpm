---
"pacquet": minor
---

`pnpm pack` now honors `--silent`, `--reporter=silent`, and `--loglevel=silent` to hide the tarball contents and summary. With `--json`, lifecycle script output and the final JSON output remain visible [#10297](https://github.com/pnpm/pnpm/issues/10297).
