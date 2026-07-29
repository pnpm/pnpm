---
"@pnpm/exec.commands": patch
"pacquet": patch
"pnpm": patch
---

`pnpm -r run "/pattern/" --no-bail` no longer exits zero when one of a project's matched scripts fails and a later one passes. The run summary carries a single status per project, and the passing script overwrote the recorded failure.
