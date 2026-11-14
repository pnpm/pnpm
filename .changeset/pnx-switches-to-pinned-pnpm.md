---
"pacquet": patch
---

`pnx` and `pnpm dlx` now switch to the pnpm version a project pins with `onFail: "download"`. They no longer fail on a `pnpm-workspace.yaml` setting that only the pinned version supports.
