---
"@pnpm/installing.commands": minor
"pnpm": minor
"pacquet": minor
---

`pnpm remove` now accepts `--trust-lockfile` and `--no-trust-lockfile`, letting a single run skip the lockfile supply-chain verification pass — or force it back on over a `trustLockfile: true` config — without editing the config file. `--trust-policy`, `--trust-policy-exclude` and `--trust-policy-ignore-after` are now accepted there too.

This does not change when verification runs: the pass still checks the lockfile as it exists before the removal, so `--trust-lockfile` skips it for every entry, not only the package being removed.
