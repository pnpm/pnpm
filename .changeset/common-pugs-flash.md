---
"@pnpm/crypto.hash": minor
---

Added a new `getTarballIntegrity` function. This function was moved from `@pnpm/local-resolver` and is used to compute the integrity hash of a local tarball `file:` dependency in the `pnpm-lock.yaml` file.
