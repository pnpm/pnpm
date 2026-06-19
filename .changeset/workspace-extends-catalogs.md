---
"@pnpm/workspace.workspace-manifest-reader": minor
"@pnpm/catalogs.config": minor
"@pnpm/installing.commands": minor
"@pnpm/config.reader": patch
"@pnpm/deps.status": patch
"pnpm": minor
---

Added an `extends` field to `pnpm-workspace.yaml` that merges catalogs from other workspace manifests into the current one.

Each `extends` entry may be:

- a directory that contains a `pnpm-workspace.yaml`,
- a direct path to a `pnpm-workspace.yaml` file (relative, absolute, or outside the workspace),
- a glob such as `packages/*` (every matching `pnpm-workspace.yaml` is merged; directories without one are skipped), or
- a path prefixed with the `<root>` token, which resolves to the monorepo root — the nearest ancestor directory that has a `pnpm-workspace.yaml` — so a package can reference the root without counting `../` segments (`<root>`, `<root>/configs/base`).

`extends` works in both directions (a root manifest can extend its packages, and a package manifest can extend the root) and is resolved recursively. Entries defined directly in the extending manifest win over inherited ones, manifests resolved later win over earlier ones, and circular references are reported as errors.

One application: with `sharedWorkspaceLockfile: false` (a dedicated lockfile per project), a package can `extends: <root>` to inherit the root catalogs, add or override its own, resolve its `catalog:` dependencies against the merged result, and record them in its own lockfile.

Related to [#10302](https://github.com/pnpm/pnpm/issues/10302).
