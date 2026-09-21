---
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm remove -r` now fails if any requested dependency is absent from all selected workspace projects. Validation respects `--save-prod`, `--save-dev`, and `--save-optional` and completes before modifying project manifests [#2319](https://github.com/pnpm/pnpm/issues/2319).
