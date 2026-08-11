---
"@pnpm/config.reader": patch
"pnpm": patch
---

Fixed `verifyDepsBeforeRun` being ignored when set to `install`, `warn`, `error`, or `prompt` through the `PNPM_CONFIG_VERIFY_DEPS_BEFORE_RUN` environment variable or the `--config.verify-deps-before-run` flag [#13816](https://github.com/pnpm/pnpm/issues/13816). Only the boolean values were accepted before, so a string value was silently dropped.
