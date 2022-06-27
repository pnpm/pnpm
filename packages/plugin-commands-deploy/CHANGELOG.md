# @pnpm/plugin-commands-deploy

## 1.0.0

### Major Changes

- 7922d6314: A new experimental command added: `pnpm deploy`. The deploy command takes copies a project from a workspace and installs all of its production dependencies (even if some of those dependencies are other projects from the workspace).

  For example, the new command will deploy the project named `foo` to the `dist` directory in the root of the workspace:

  ```
  pnpm --filter=foo deploy dist
  ```

### Patch Changes

- Updated dependencies [7922d6314]
  - @pnpm/fs.indexed-pkg-importer@1.0.0
  - @pnpm/plugin-commands-installation@10.3.3
