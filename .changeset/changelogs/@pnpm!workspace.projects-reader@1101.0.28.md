## 1101.0.28

### Patch Changes

- A command run in a project that the workspace does not include now acts on that project alone. A project is outside the workspace when it has a manifest of its own and no pattern in the `packages` setting selects it, or when a `!` pattern excludes it. A directory with no manifest of its own, such as a package's source directory, still belongs to the workspace. `pnpm install` in an excluded project used to install every project in the workspace [#3561](https://github.com/pnpm/pnpm/issues/3561).

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.28
  - @pnpm/workspace.project-manifest-reader@1100.0.29
