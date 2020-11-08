# @pnpm/parse-cli-args

## 3.2.1

### Patch Changes

- Updated dependencies [0c5f1bcc9]
  - @pnpm/error@1.4.0

## 3.2.0

### Minor Changes

- 092f8dd83: When --workspace-root is used, the working directory is changed to the root of the workspace.

## 3.1.2

### Patch Changes

- 9f5803187: Update nopt to v5.

## 3.1.1

### Patch Changes

- Updated dependencies [999f81305]
  - @pnpm/find-workspace-dir@1.0.1

## 3.1.0

### Minor Changes

- be0a3db9b: Allow unknown options that are prefixed with "config."

## 3.0.1

### Patch Changes

- a2ef8084f: Use the same versions of dependencies across the pnpm monorepo.

## 3.0.0

### Major Changes

- 65b4d07ca: move recursive for workspace check to pnpm main

## 2.1.0

### Minor Changes

- 09b777f8d: New option added: `fallbackCommand`. If set, this command is added to the beginning of any unknown query.

## 2.0.0

### Major Changes

- 561f38955: `unknownOptions` in the result object is a `Map` instead of an `Array`.

  `unknownOptions` is a map of unknown options to options that are similar to the unknown options.

## 1.1.0

### Minor Changes

- 0e8daafe4: The mapping of CLI option shorthands may use arrays of string.

## 1.0.1

### Patch Changes

- @pnpm/find-workspace-dir@1.0.1
