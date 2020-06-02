# @pnpm/store-connection-manager

## 0.3.4

### Patch Changes

- @pnpm/default-resolver@8.0.1
- @pnpm/package-store@9.0.2
- @pnpm/server@8.0.1
- @pnpm/default-fetcher@6.0.3

## 0.3.3

### Patch Changes

- Updated dependencies [1dcfecb36]
  - @pnpm/server@8.0.1

## 0.3.2

### Patch Changes

- Updated dependencies [ffddf34a8]
- Updated dependencies [429c5a560]
  - @pnpm/config@9.1.0
  - @pnpm/package-store@9.0.1
  - @pnpm/default-fetcher@6.0.2
  - @pnpm/server@8.0.0

## 0.3.1

### Patch Changes

- @pnpm/default-fetcher@6.0.1

## 0.3.0

### Minor Changes

- da091c711: Remove state from store. The store should not store the information about what projects on the computer use what dependencies. This information was needed for pruning in pnpm v4. Also, without this information, we cannot have the `pnpm store usages` command. So `pnpm store usages` is deprecated.
- b6a82072e: Using a content-addressable filesystem for storing packages.
- 45fdcfde2: Locking is removed.

### Patch Changes

- Updated dependencies [b5f66c0f2]
- Updated dependencies [242cf8737]
- Updated dependencies [cbc2192f1]
- Updated dependencies [f516d266c]
- Updated dependencies [ecf2c6b7d]
- Updated dependencies [da091c711]
- Updated dependencies [a7d20d927]
- Updated dependencies [e11019b89]
- Updated dependencies [802d145fc]
- Updated dependencies [b6a82072e]
- Updated dependencies [802d145fc]
- Updated dependencies [c207d994f]
- Updated dependencies [45fdcfde2]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
- Updated dependencies [919103471]
  - @pnpm/package-store@9.0.0
  - @pnpm/server@8.0.0
  - @pnpm/config@9.0.0
  - @pnpm/default-fetcher@6.0.0
  - @pnpm/cli-meta@1.0.0
  - @pnpm/default-resolver@7.4.10
  - @pnpm/error@1.2.1

## 0.3.0-alpha.5

### Minor Changes

- 45fdcfde2: Locking is removed.

### Patch Changes

- Updated dependencies [242cf8737]
- Updated dependencies [a7d20d927]
- Updated dependencies [45fdcfde2]
- Updated dependencies [a5febb913]
- Updated dependencies [a5febb913]
  - @pnpm/config@9.0.0-alpha.2
  - @pnpm/package-store@9.0.0-alpha.5
  - @pnpm/server@8.0.0-alpha.5
  - @pnpm/default-fetcher@5.1.19-alpha.5

## 0.3.0-alpha.4

### Minor Changes

- da091c71: Remove state from store. The store should not store the information about what projects on the computer use what dependencies. This information was needed for pruning in pnpm v4. Also, without this information, we cannot have the `pnpm store usages` command. So `pnpm store usages` is deprecated.

### Patch Changes

- Updated dependencies [ecf2c6b7]
- Updated dependencies [da091c71]
  - @pnpm/package-store@9.0.0-alpha.4
  - @pnpm/server@8.0.0-alpha.4
  - @pnpm/default-fetcher@5.1.19-alpha.4
  - @pnpm/cli-meta@1.0.0-alpha.0
  - @pnpm/config@8.3.1-alpha.1
  - @pnpm/default-resolver@7.4.10-alpha.2

## 0.3.0-alpha.3

### Patch Changes

- Updated dependencies [b5f66c0f2]
  - @pnpm/package-store@9.0.0-alpha.3
  - @pnpm/server@8.0.0-alpha.3
  - @pnpm/config@8.3.1-alpha.0
  - @pnpm/default-resolver@7.4.10-alpha.1
  - @pnpm/default-fetcher@5.1.19-alpha.3

## 0.2.32-alpha.2

### Patch Changes

- Updated dependencies [c207d994f]
- Updated dependencies [919103471]
  - @pnpm/package-store@9.0.0-alpha.2
  - @pnpm/server@8.0.0-alpha.2
  - @pnpm/default-fetcher@5.1.19-alpha.2
  - @pnpm/default-resolver@7.4.10-alpha.0

## 0.3.0-alpha.1

### Patch Changes

- Updated dependencies [4f62d0383]
  - @pnpm/package-store@9.0.0-alpha.1
  - @pnpm/server@7.0.5-alpha.1
  - @pnpm/default-fetcher@5.1.19-alpha.1

## 0.3.0-alpha.0

### Minor Changes

- 91c4b5954: Using a content-addressable filesystem for storing packages.

### Patch Changes

- Updated dependencies [91c4b5954]
  - @pnpm/default-fetcher@6.0.0-alpha.0
  - @pnpm/package-store@9.0.0-alpha.0
  - @pnpm/server@8.0.0-alpha.0

## 0.2.31

### Patch Changes

- 907c63a48: Update `@pnpm/store-path`.
- 907c63a48: Dependencies updated.
- 907c63a48: Use `fs.mkdir` instead of `make-dir`.
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
  - @pnpm/package-store@8.1.0
  - @pnpm/server@7.0.4
  - @pnpm/default-fetcher@5.1.18
  - @pnpm/default-resolver@7.4.9
