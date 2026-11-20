---
"@pnpm/pnpr": minor
---

`artifacts.orgs` declares who may read and who may publish what an organization owns: its signed artifacts, its Cargo compilation cache, and its pipeline run records. It replaces `artifacts.compilerCaches` and `pipeline.workspaces`, which are no longer accepted.

Signed artifacts no longer require the organization's name to match the account that reads or publishes them. Pipeline runs are recorded under an organization, and run URLs include it. Runs recorded by earlier versions are not listed.
