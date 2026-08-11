---
"@pnpm/config.reader": patch
"pnpm": patch
---

Fixed an issue where `verifyDepsBeforeRun` could not be configured with string values (`error`, `warn`, `install`, `prompt`) via environment variables (e.g. `PNPM_CONFIG_VERIFY_DEPS_BEFORE_RUN`).
