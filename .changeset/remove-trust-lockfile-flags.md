---
"@pnpm/installing.commands": minor
"pnpm": minor
"pacquet": minor
---

`pnpm remove` and `pnpm update` now accept `--trust-lockfile`, `--no-trust-lockfile`, `--trust-policy`, `--trust-policy-exclude` and `--trust-policy-ignore-after`, the same flags `pnpm install` and `pnpm add` take, so the supply-chain settings can be overridden for a single run. `pnpm remove` verifies the lockfile against the active policies the way `pnpm install` does, and `--trust-lockfile` skips that pass for every entry, not only the package being removed.

The Rust CLI now also honors `--config.trust-lockfile=<value>`, and accepts the bare `--trust-lockfile` / `--no-trust-lockfile` spelling on the commands that previously took the setting from the config file alone.
