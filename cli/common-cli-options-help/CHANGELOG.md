# @pnpm/common-cli-options-help

## 0.9.0

### Minor Changes

- 542014839: Node.js 12 is not supported.

## 0.8.0

### Minor Changes

- 927c4a089: A new option `--aggregate-output` for `append-only` reporter is added. It aggregates lifecycle logs output for each command that is run in parallel, and only prints command logs when command is finished.

  Related discussion: [#4070](https://github.com/pnpm/pnpm/discussions/4070).

## 0.7.1

### Patch Changes

- 1efaaf706: `OUTPUT_OPTIONS` added.

## 0.7.0

### Minor Changes

- fe5688dc0: Add option 'changed-files-ignore-pattern' to ignore changed files by glob patterns when filtering for changed projects since the specified commit/branch.

## 0.6.0

### Minor Changes

- c2a71e4fd: New CLI option added: `use-stderr`. When set, all the output is written to stderr.

## 0.5.0

### Minor Changes

- dfdf669e6: Add new cli arg --filter-prod. --filter-prod acts the same as --filter, but it omits devDependencies when building dependencies

## 0.4.0

### Minor Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

## 0.3.1

### Patch Changes

- a5e9d903c: Update help for filters. Some of the filtering patterns should be escaped in zsh.

## 0.3.0

### Minor Changes

- 1ec47db33: New CLI option added: `--test-pattern`.

## 0.2.0

### Minor Changes

- 092f8dd83: New universal option added: -w, --workspace-root.

## 0.1.6

### Minor Changes

- ffddf34a8: Add `--stream` option description to the `UNIVERSAL_OPTIONS` array.
