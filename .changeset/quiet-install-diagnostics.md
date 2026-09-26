---
"@pnpm/cli.default-reporter": patch
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

With the default and append-only reporters, installs with `--loglevel warn` or `--loglevel error` now print the full output of a failed install script. The output of successful scripts, including the root project's own install hooks, stays hidden. With `--loglevel warn`, pnpm also prints supply-chain verification verdicts and ignored build script warnings.
