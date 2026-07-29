## 1100.1.21

### Patch Changes

- The `Workspace` column of `pnpm update --interactive` now falls back to the project's path when its `name` is only whitespace, as it already did for a missing or empty one — all three render an equally blank label otherwise.

- The `Workspace` column of `pnpm update --interactive` is more informative in two cases. A dependency outdated at the same version in several workspace projects is offered as one choice, since selecting it updates every project — that choice now names all of them instead of only the first. And a workspace project without a `name` is now labelled with its path rather than left blank, so several unnamed projects can be told apart.

- Updated dependencies:
  - @pnpm/config.version-policy@1100.1.10
  - @pnpm/deps.path@1100.0.12
  - @pnpm/hooks.read-package-hook@1100.2.0
  - @pnpm/installing.client@1100.3.0
  - @pnpm/lockfile.fs@1100.1.15
  - @pnpm/lockfile.utils@1100.1.6
  - @pnpm/pkg-manifest.utils@1100.2.13
  - @pnpm/resolving.npm-resolver@1102.1.9
  - @pnpm/types@1101.7.0
