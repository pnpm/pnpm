---
"@pnpm/workspace.projects-reader": patch
pnpm: patch
---

The warning shown when a non-root workspace package contains a `"resolutions"` field now directs users to the correct fix for pnpm v11: the `"overrides"` field in `pnpm-workspace.yaml` at the workspace root. The previous message told users to "configure `resolutions` at the root of the workspace", which is no longer how pnpm wires version overrides. Fixes [#11757](https://github.com/pnpm/pnpm/issues/11757).
