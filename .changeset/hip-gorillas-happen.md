---
"@pnpm/common-cli-options-help": minor
"@pnpm/config": minor
"@pnpm/default-reporter": minor
"pnpm": minor
---

A new option `--aggregate-output` for `append-only` reporter is added. It aggregates lifecycle logs output for each command that is run in parallel, and only prints command logs when command is finished.

Related discussion: [#4070](https://github.com/pnpm/pnpm/discussions/4070).
