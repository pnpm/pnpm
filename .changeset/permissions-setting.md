---
"pacquet": minor
---

Added the `permissions` setting, which records what each dependency may do. Its `build` capability works like `allowBuilds` and takes precedence over it. `pnpm approve-builds` writes to `permissions` when `pnpm-workspace.yaml` already has it, and to `allowBuilds` otherwise.

Added `pnpm permissions`, which lists the granted and denied permissions and the packages awaiting approval. `pnpm approve` reviews build scripts and agent skills in one prompt [pnpm/rfcs#36](https://github.com/pnpm/rfcs/pull/36).
