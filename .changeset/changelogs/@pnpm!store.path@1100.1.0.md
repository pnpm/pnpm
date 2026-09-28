## 1100.1.0

### Minor Changes

- pnpm now warns when it cannot hard link packages from an existing store in the pnpm home directory and uses a store on the project's filesystem instead. This can happen when the project is on another filesystem, such as a bind-mounted workspace in a container. The warning names both stores and suggests setting `storeDir` [#14505](https://github.com/pnpm/pnpm/issues/14505).

### Patch Changes

- Updated dependencies:
  - @pnpm/error@1100.2.1
