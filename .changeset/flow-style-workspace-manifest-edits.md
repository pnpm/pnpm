---
"pacquet": patch
---

Settings written to a `pnpm-workspace.yaml` block that uses inline (flow) YAML — `catalog: { foo: ^1.0.0 }`, `overrides: { foo: 1.0.0 }`, `minimumReleaseAgeExclude: [foo@1.0.0]` — are now edited in place instead of failing or corrupting the file. `pnpm audit`, `pnpm link`, `pnpm approve-builds`, `pnpm patch`, `pnpm add --config`, and catalog updates all keep the block's flow style, its other entries, and its comments [#14108](https://github.com/pnpm/pnpm/issues/14108).
