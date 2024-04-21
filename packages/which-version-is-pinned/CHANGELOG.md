# @pnpm/which-version-is-pinned

## 6.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

## 5.0.1

### Patch Changes

- 4fc497882: When updating dependencies, preserve the range prefix in aliased dependencies. So `npm:foo@1.0.0` becomes `npm:foo@1.1.0`.

## 5.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

## 4.0.0

### Major Changes

- f884689e0: Require `@pnpm/logger` v5.

## 3.0.0

### Major Changes

- f5621a42c: A new value `rolling` for option `save-workspace-protocol`. When selected, pnpm will save workspace versions using a rolling alias (e.g. `"foo": "workspace:^"`) instead of pinning the current version number (e.g. `"foo": "workspace:^1.0.0"`). Usage example:

  ```
  pnpm --save-workspace-protocol=rolling add foo
  ```

## 2.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

## 1.0.0

### Major Changes

- ae32d313e: Initial release.
