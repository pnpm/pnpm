## 1100.3.14

### Patch Changes

- The workspace project graph now follows `overrides`. An override that points a dependency at a workspace package, such as `foo: workspace:*`, makes that package a dependency of every project whose declaration the override matches, even with `linkWorkspacePackages` off. `pnpm install` now runs that package's lifecycle scripts before the scripts of the projects depending on it. `pnpm -r run` orders the projects the same way. `--filter` selectors that follow dependencies now see the edge.

- Internal type-safety cleanup with no change in behavior.

- Updated dependencies:
  - @pnpm/config.parse-overrides@1100.1.9
  - @pnpm/error@1100.2.3
  - @pnpm/resolving.local-resolver@1101.2.11
  - @pnpm/types@1102.1.5
