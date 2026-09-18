---
"pacquet": patch
---

pnpm now reports an unknown task setting in `pnpm-workspace.yaml` and carries on. It used to refuse to start, so a project could not use a task setting that only the pnpm version its `packageManager` pins reads. The setting is still an error when the running pnpm is that pinned version.
