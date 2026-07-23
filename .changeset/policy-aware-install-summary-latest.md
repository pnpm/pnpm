---
"pnpm": patch
"pacquet": patch
---

The install summary no longer prints `(X is available)` for a version the active `minimumReleaseAge` policy itself held back. The hint now compares against the policy-aware latest (highest mature version) rather than the raw registry `dist-tags.latest`, so a default 24-hour cooldown no longer advertises the version pnpm just refused to install. Closes pnpm/pnpm#11698.
