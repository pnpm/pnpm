---
"@pnpm/cli.common-cli-options-help": patch
"@pnpm/installing.commands": patch
"pnpm": patch
---

Clarified in CLI help that the pnpm store is trusted shared state and store integrity checks are corruption detection, not a tamper boundary for untrusted store writers.
