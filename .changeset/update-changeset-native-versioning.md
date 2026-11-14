---
"@pnpm/installing.commands": patch
"pacquet": patch
"pnpm": patch
---

`pnpm update --changeset` now generates a changeset when a `.changeset` directory exists or `versioning` is configured in `pnpm-workspace.yaml`, without requiring `.changeset/config.json`. It also respects `versioning.ignore` alongside `.changeset/config.json`.
