---
"@pnpm/cli.default-reporter": minor
---

Added a `pnpm-render` bin that renders pnpm-shaped NDJSON read from stdin, so the same renderer can be used to format output from external tools that emit `pnpm:*` log records (e.g. `pacquet install --reporter=ndjson 2>&1 >/dev/null | pnpm-render`). An optional first positional argument sets the command name (defaults to `install`).
