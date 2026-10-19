---
"@pnpm/config.reader": patch
"@pnpm/workspace.workspace-manifest-reader": minor
"pnpm": patch
---

`pmOnFail` and `lockfileDir` in `pnpm-workspace.yaml` now stay in force when another setting in that file cannot be read, so pnpm no longer switches to the pinned version against `pmOnFail: ignore`.
