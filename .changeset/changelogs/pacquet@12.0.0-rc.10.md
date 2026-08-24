## 12.0.0-rc.10

### Patch Changes

- Fixed pnpm v11 incorrectly reporting `confirmModulesPurge` as unrecognized when set in `pnpm-workspace.yaml`. The Rust CLI now identifies the unsupported option as a pnpm v11 setting instead of suggesting an unrelated setting.

- A `+<algorithm>.<hash>` build in a `devEngines.packageManager` version no longer makes `pnpm install --frozen-lockfile` fail with `ERR_PNPM_FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE` on a lockfile a plain install kept rewriting identically [#14124](https://github.com/pnpm/pnpm/issues/14124).

- The built-in compatibility database no longer adds dependencies that were detected by static analysis of published packages. Those entries named packages that are only imported for their types, so installing them was at best unnecessary and at worst broke the dependent: `@typescript-eslint/types` gained a `typescript` dependency resolved to the newest release, which put TypeScript 7 under older `@typescript-eslint` versions and made ESLint fail with "Cannot read properties of undefined (reading 'Intrinsic')". The database keeps its `@yarnpkg/extensions` entries and pnpm's own curated ones.

- `pnpm install --frozen-lockfile` no longer fails with `ERR_PNPM_FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE` when the pinned pnpm version recorded in `pnpm-lock.yaml` has to be re-resolved before it can be installed. It runs the pnpm version the lockfile pins and leaves the lockfile unchanged [#14124](https://github.com/pnpm/pnpm/issues/14124).

- Under `nodeLinker: hoisted`, peer-resolution variants of an injected directory dependency (a `file:` snapshot) are materialized as separate copies again instead of collapsing onto the first-seen variant. Each copy keeps its own peer-resolved dependency set, so a project pinning one peer version no longer resolves another project's variant — Bit root components with conflicting peers across injected copies rely on this.

- Fixed `pnpm install --merge-git-branch-lockfiles --frozen-lockfile` failing with `ERR_PNPM_OUTDATED_LOCKFILE` when a branch lockfile predates the removal of a dependency, or its move to another dependency group [#13966](https://github.com/pnpm/pnpm/issues/13966). A dependency that no project declares anymore is no longer reinstated by the merge, and the packages it was the only path to are dropped with it.

- Record the pnpm version a project pins even when the install has nothing else to do. Adding a `devEngines.packageManager` (or `packageManager`) pin to a project whose dependencies are already installed left `packageManagerDependencies` unwritten, so `pnpm install --frozen-lockfile` failed with `ERR_PNPM_FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE` while a plain `pnpm install` reported "Already up to date" without recording it [#14124](https://github.com/pnpm/pnpm/issues/14124).

- `pnpm install --frozen-lockfile` no longer fails when `pnpm-lock.yaml` records the pinned pnpm version alongside an engine package the running pnpm does not install it from. An entry pinning another version is still refused, and a plain install rewrites the block [#14124](https://github.com/pnpm/pnpm/issues/14124).
