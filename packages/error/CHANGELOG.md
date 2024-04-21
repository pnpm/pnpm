# @pnpm/error

## 6.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

### Patch Changes

- 3ded840: Print the right error code when a package fails to be added to the store [#7679](https://github.com/pnpm/pnpm/issues/7679).
- Updated dependencies [c692f80]
- Updated dependencies [43cdd87]
- Updated dependencies [d381a60]
  - @pnpm/constants@8.0.0

## 5.0.2

### Patch Changes

- Updated dependencies [302ebffc5]
  - @pnpm/constants@7.1.1

## 5.0.1

### Patch Changes

- Updated dependencies [9c4ae87bd]
  - @pnpm/constants@7.1.0

## 5.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [eceaa8b8b]
  - @pnpm/constants@7.0.0

## 4.0.1

### Patch Changes

- Updated dependencies [3ebce5db7]
  - @pnpm/constants@6.2.0

## 4.0.0

### Major Changes

- 043d988fc: Breaking change to the API. Defaul export is not used.
- f884689e0: Require `@pnpm/logger` v5.

## 3.1.0

### Minor Changes

- e8a631bf0: Add new optional field: prefix.

## 3.0.1

### Patch Changes

- Updated dependencies [1267e4eff]
  - @pnpm/constants@6.1.0

## 3.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [542014839]
  - @pnpm/constants@6.0.0

## 2.1.0

### Minor Changes

- 70ba51da9: Add new error object: LockfileMissingDependencyError.

## 2.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

## 1.4.0

### Minor Changes

- 0c5f1bcc9: Every error object has an optional "attempts" field.

## 1.3.1

### Patch Changes

- 75a36deba: Report auth info on 404 errors as well.

## 1.3.0

### Minor Changes

- 6d480dd7a: A new error class added for throwing fetch errors: FetchError.

## 1.2.1
