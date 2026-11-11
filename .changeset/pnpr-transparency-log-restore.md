---
"@pnpm/pnpr": patch
---

pnpr now re-imports transparency log entries from stored entry bundles when the metadata database is restored from an earlier snapshot than hosted storage. Rolled-back package versions from those entries cannot be republished, and pnpr refuses to start or sign checkpoints when stored log state diverges from the database [zkochan/pnpm-tasks#65](https://github.com/zkochan/pnpm-tasks/issues/65).
