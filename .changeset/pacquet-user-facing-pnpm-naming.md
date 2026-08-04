---
"pacquet": patch
---

Error messages and `--help` text now refer to the CLI as `pnpm` instead of the internal `pacquet` name. Several messages previously suggested commands like `pacquet install --frozen-lockfile`, which is not a command users can run, and `pnpm add --help` documented the virtual store directory default as `node_modules/.pacquet` rather than the actual `node_modules/.pnpm`.
