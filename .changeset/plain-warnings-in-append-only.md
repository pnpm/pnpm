---
"@pnpm/cli.default-reporter": patch
"pnpm": patch
---

The ignored build scripts warning and the update notice are printed as plain lines when output is not a terminal, in CI, or with `--reporter append-only`. They were drawn inside a box that broke apart in CI logs [#9421](https://github.com/pnpm/pnpm/issues/9421).
