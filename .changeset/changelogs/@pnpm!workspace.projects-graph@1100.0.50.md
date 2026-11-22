## 1100.0.50

### Patch Changes

- The workspace project graph now follows `overrides`. An override that points a dependency at a workspace package, such as `foo: workspace:*`, makes that package a dependency of every project whose declaration the override matches, even with `linkWorkspacePackages` off. `pnpm install` now runs that package's lifecycle scripts before the scripts of the projects depending on it. `pnpm -r run` orders the projects the same way. `--filter` selectors that follow dependencies now see the edge.

- A `[<since>]` filter such as `--filter "...[origin/main]"` now selects a project whose dependency an `overrides` entry points at a changed catalog entry. For example, with `lib: catalog:local` in `overrides`, changing `lib` in the `local` catalog selects every project that depends on `lib`.

- Updated dependencies:
  - @pnpm/config.parse-overrides@1100.1.9
  - @pnpm/hooks.read-package-hook@1100.3.14
  - @pnpm/resolving.npm-resolver@1104.2.12
  - @pnpm/types@1102.1.5
