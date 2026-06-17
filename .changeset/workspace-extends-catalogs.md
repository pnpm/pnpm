---
"@pnpm/workspace.workspace-manifest-reader": minor
"@pnpm/catalogs.config": minor
"@pnpm/installing.commands": minor
"@pnpm/config.reader": patch
"@pnpm/deps.status": patch
"pnpm": minor
---

Added an `extends` field to `pnpm-workspace.yaml`. When `shared-workspace-lockfile` is `false` (a dedicated lockfile per project), a project may have its own `pnpm-workspace.yaml` that `extends` another workspace manifest (typically the workspace root) by path. The project then inherits the extended catalogs, may add or override catalog entries of its own, and resolves its `catalog:` dependencies against the merged catalogs — recording them in its own lockfile.

Entries defined directly in the extending (child) manifest take precedence over the inherited ones, and `extends` is resolved recursively with circular references reported as errors [#10302](https://github.com/pnpm/pnpm/issues/10302).
