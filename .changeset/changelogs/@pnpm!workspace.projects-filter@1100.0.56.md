## 1100.0.56

### Patch Changes

- The workspace project graph now follows `overrides`. An override that points a dependency at a workspace package, such as `foo: workspace:*`, makes that package a dependency of every project whose declaration the override matches, even with `linkWorkspacePackages` off. `pnpm install` now runs that package's lifecycle scripts before the scripts of the projects depending on it. `pnpm -r run` orders the projects the same way. `--filter` selectors that follow dependencies now see the edge.

- A `[<since>]` filter such as `--filter "...[origin/main]"` now selects a project whose dependency an `overrides` entry points at a changed catalog entry. For example, with `lib: catalog:local` in `overrides`, changing `lib` in the `local` catalog selects every project that depends on `lib`.

- Updated dependencies:
  - @pnpm/catalogs.config@1100.0.11
  - @pnpm/error@1100.2.3
  - @pnpm/workspace.projects-graph@1100.0.50
  - @pnpm/workspace.projects-reader@1101.1.9
  - @pnpm/workspace.workspace-manifest-reader@1100.3.4
