## 12.5.1

### Patch Changes

- pnpm now reports an unknown task setting in `pnpm-workspace.yaml` and carries on. It used to refuse to start, so a project could not use a task setting that only the pnpm version its `packageManager` pins reads. The setting is still an error when the running pnpm is that pinned version.

- Python interpreter installation now retries historical release metadata requests. It caches the release list for up to 24 hours and refreshes it once after a lookup miss. When a release omits the current platform, the search samples at most eight other releases before reporting that the lookup is inconclusive.

- Python `registries` entries now route packages by exact names or trailing-prefix patterns in `packages`. Registry declaration order no longer affects resolution. A matched package resolves exclusively from its assigned registry, including transitive and build dependencies. Use `packages: ["*"]` to declare the default index.

- `pnpm install` no longer fails with "Too many levels of symbolic links" when a Cargo configuration file above the workspace is a symlink, such as a `~/.cargo/config.toml` linked from a dotfiles repository.

- `pnpm install` now returns "Already up to date" in a workspace where `dedupeDirectDeps` left a project without a `node_modules` directory of its own. Such a project forced a full install on every run.

- `pnpm install` no longer refuses the repeat-install fast path just because a changed `pnpm-lock.yaml` is 16 MiB or larger. Such a lockfile forced a full install on the run after every change.
