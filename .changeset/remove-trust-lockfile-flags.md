---
"@pnpm/installing.commands": minor
"pnpm": minor
"pacquet": minor
---

`pnpm remove` now accepts the `--trust-lockfile`, `--trust-policy`, `--trust-policy-exclude` and `--trust-policy-ignore-after` flags, matching `pnpm install` and `pnpm add`. They were previously rejected as unknown options, so these settings could only reach `pnpm remove` through `--config.<name>=<value>` or the config file. Lockfile verification itself is unchanged.
