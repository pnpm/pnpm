## 1100.2.0

### Minor Changes

- When the registry stops sending data for longer than `fetchTimeout`, pnpm now reports that the metadata or tarball request timed out. Previously the error did not mention the timeout [#3646](https://github.com/pnpm/pnpm/issues/3646).
