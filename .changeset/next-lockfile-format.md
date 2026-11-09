---
"pacquet": minor
---

Added the experimental `lockfile.format: v13` setting. With it, pnpm writes `pnpm-lock.yaml` with snapshots keyed by package name and version, and with each snapshot's peer dependency versions recorded as its properties. Updating a peer dependency then changes only the lines that record its version. pnpm reads lockfiles in both formats whatever the setting is.
