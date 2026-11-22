---
"pnpm": minor
---

Added `pnpm doctor`, which diagnoses the pnpm installation and the environment it runs in: the versions and install method, whether the global bin directory is on `PATH`, whether the store and cache are writable, which link strategies (reflink, hardlink, symlink) the store's filesystem supports, registry connectivity, and an offline `file:` install that exercises the resolve/store/link path end to end. Each check reports how to fix what it finds, and the command exits non-zero when any check fails.

Use `--offline` to skip the checks that need network access, `--json` for machine-readable output, and `--benchmark` to time the filesystem and install checks.
