# @pnpm/parse-cli-args

## 8.0.1

### Patch Changes

- Updated dependencies [a7aef51]
  - @pnpm/error@6.0.1
  - @pnpm/find-workspace-dir@7.0.1

## 8.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

### Patch Changes

- Updated dependencies [3ded840]
- Updated dependencies [43cdd87]
  - @pnpm/error@6.0.0
  - @pnpm/find-workspace-dir@7.0.0

## 7.0.4

### Patch Changes

- 1ce2dd13a: Update didyoumean2 to v6.

## 7.0.3

### Patch Changes

- 32679f0ad: Don't ignore empty strings in params [#6594](https://github.com/pnpm/pnpm/issues/6594).

## 7.0.2

### Patch Changes

- @pnpm/error@5.0.2
- @pnpm/find-workspace-dir@6.0.2

## 7.0.1

### Patch Changes

- @pnpm/error@5.0.1
- @pnpm/find-workspace-dir@6.0.1

## 7.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [eceaa8b8b]
  - @pnpm/find-workspace-dir@6.0.0
  - @pnpm/error@5.0.0

## 6.0.1

### Patch Changes

- @pnpm/error@4.0.1
- @pnpm/find-workspace-dir@5.0.1

## 6.0.0

### Major Changes

- f884689e0: Require `@pnpm/logger` v5.

### Patch Changes

- Updated dependencies [043d988fc]
- Updated dependencies [f884689e0]
  - @pnpm/error@4.0.0
  - @pnpm/find-workspace-dir@5.0.0

## 5.0.3

### Patch Changes

- Updated dependencies [e8a631bf0]
  - @pnpm/error@3.1.0
  - @pnpm/find-workspace-dir@4.0.3

## 5.0.2

### Patch Changes

- Updated dependencies [6434a8291]
  - @pnpm/find-workspace-dir@4.0.2

## 5.0.1

### Patch Changes

- @pnpm/error@3.0.1
- @pnpm/find-workspace-dir@4.0.1

## 5.0.0

### Major Changes

- c35ac786b: When using `pnpm run <script>`, all command line arguments after the script name are now passed to the script's argv, even `--`. For example, `pnpm run echo --hello -- world` will now pass `--hello -- world` to the `echo` script's argv. Previously flagged arguments (e.g. `--silent`) were interpreted as pnpm arguments unless `--` came before it.
- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [542014839]
  - @pnpm/error@3.0.0
  - @pnpm/find-workspace-dir@4.0.0

## 4.4.1

### Patch Changes

- Updated dependencies [70ba51da9]
  - @pnpm/error@2.1.0
  - @pnpm/find-workspace-dir@3.0.2

## 4.4.0

### Minor Changes

- 8fe8f5e55: New CLI option: `--ignore-workspace`. When used, pnpm ignores any workspace configuration found in the current or parent directories.

## 4.3.0

### Minor Changes

- 06f127503: A new option added: `escapeArgs`. `escapeArgs` is an array of arguments that stop arguments parsing.
  By default, everything after `--` is not parsed as key-value. This option allows to add new keywords to stop parsing.

## 4.2.2

### Patch Changes

- 05ed9ea63: Update didyoumean2 to v5.

## 4.2.1

### Patch Changes

- 22f841039: The `--help` option should not convert the command to `help` if the command is unknown. So `pnpm eslint -h` is not parsed as `pnpm help eslint`.

## 4.2.0

### Minor Changes

- 209c14235: A new property is returned in the result: fallbackCommandUsed. It is true when an unknown command was used, so the fallback command had to be used instead.

## 4.1.1

### Patch Changes

- Updated dependencies [6e8cedb79]
  - @pnpm/find-workspace-dir@3.0.1

## 4.1.0

### Minor Changes

- dfdf669e6: Add new cli arg --filter-prod. --filter-prod acts the same as --filter, but it omits devDependencies when building dependencies

## 4.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Patch Changes

- Updated dependencies [97b986fbc]
  - @pnpm/error@2.0.0
  - @pnpm/find-workspace-dir@3.0.0

## 3.2.2

### Patch Changes

- Updated dependencies [ec37069f2]
  - @pnpm/find-workspace-dir@2.0.0

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
