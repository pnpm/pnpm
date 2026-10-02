## 12.9.0

### Minor Changes

- `install()` accepts a `readPackageHookChecksum` option. The checksum is recorded as the lockfile's `pnpmfileChecksum`. While it stays the same, installs with a `readPackageHook` reuse the resolved dependencies in the lockfile. Without it, an install with a hook that has to update the lockfile resolves all dependencies again.

### Patch Changes

- The install summary shows a `link:` dependency as `+ name <- path` again, and the Node.js API's `hideLinkedPkgsDiff` reporter option leaves matching linked dependencies out of the summary.
