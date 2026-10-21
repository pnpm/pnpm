## 1100.3.9

### Patch Changes

- pnpm now measures a `pnpm.overrides` entry written as a `workspace:` path, such as `workspace:./packages/foo`, from the directory holding `pnpm-workspace.yaml`. It used to be measured from each package the override rewrote. The install then linked a directory that does not exist, or failed if workspace packages were injected [#17010](https://github.com/pnpm/pnpm/issues/17010).

- Updated dependencies:
  - @pnpm/resolving.local-resolver@1101.2.7
  - @pnpm/types@1102.1.2
