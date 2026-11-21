---
"pacquet": minor
---

Added the `referenceTime` setting. `minimumReleaseAge` and `trustPolicyIgnoreAfter` count back from this RFC 3339 time, both when pnpm resolves versions and when it verifies a lockfile. Set it in `pnpm-workspace.yaml` or with `PNPM_CONFIG_REFERENCE_TIME`.
