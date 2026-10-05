---
"@pnpm/config.reader": patch
"pnpm": patch
"pacquet": patch
---

A project-level `.npmrc` file can no longer disable `strict-ssl`. TLS certificate validation settings are now honored only from trusted user configuration, environment variables, or CLI options.
