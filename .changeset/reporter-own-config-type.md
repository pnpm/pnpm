---
"@pnpm/cli.default-reporter": patch
"pnpm": patch
---

The default reporter no longer depends on `@pnpm/config.reader` at runtime: it declares its own minimal `ReporterPnpmConfig` type for the config fields it reads. Hosts that embed the reporter no longer pull in the config-reader dependency tree.
