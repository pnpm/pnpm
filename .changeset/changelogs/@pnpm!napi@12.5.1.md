## 12.5.1

### Patch Changes

- `pnpm install` now returns "Already up to date" in a workspace where `dedupeDirectDeps` left a project without a `node_modules` directory of its own. Such a project forced a full install on every run.

- `pnpm install` no longer refuses the repeat-install fast path just because a changed `pnpm-lock.yaml` is 16 MiB or larger. Such a lockfile forced a full install on the run after every change.
