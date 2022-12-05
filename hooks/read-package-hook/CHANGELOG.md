# @pnpm/hooks.read-package-hook

## 2.0.7

### Patch Changes

- 924eca293: It should be possible to override a dependency with a local package using relative path from the workspace root directory [#5493](https://github.com/pnpm/pnpm/issues/5493).
- Updated dependencies [a9d59d8bc]
  - @pnpm/parse-wanted-dependency@4.0.1
  - @pnpm/parse-overrides@3.0.1

## 2.0.6

### Patch Changes

- Updated dependencies [969f8a002]
  - @pnpm/matcher@4.0.1

## 2.0.5

### Patch Changes

- Updated dependencies [702e847c1]
  - @pnpm/types@8.9.0

## 2.0.4

### Patch Changes

- 0da2f0412: Update dependencies.

## 2.0.3

### Patch Changes

- da22f0c1f: Version overrider should have higher priority then custom read package hook from `.pnpmfile.cjs`.

## 2.0.2

### Patch Changes

- 0fe927215: The custom hooks should be executed after the peer dependency patcher hook.

## 2.0.1

### Patch Changes

- Updated dependencies [844e82f3a]
  - @pnpm/types@8.8.0

## 2.0.0

### Major Changes

- f884689e0: Require `@pnpm/logger` v5.

### Patch Changes

- Updated dependencies [f884689e0]
  - @pnpm/matcher@4.0.0
  - @pnpm/parse-overrides@3.0.0
  - @pnpm/parse-wanted-dependency@4.0.0

## 1.0.2

### Patch Changes

- f4813c487: Update @yarnpkg/extensions.
- 8c3a0b236: The readPackageHooks should always get the project directory as the second argument.

## 1.0.1

### Patch Changes

- @pnpm/parse-overrides@2.0.4

## 1.0.0

### Major Changes

- 51566e34b: First release.

### Patch Changes

- Updated dependencies [abb41a626]
- Updated dependencies [d665f3ff7]
  - @pnpm/matcher@3.2.0
  - @pnpm/types@8.7.0
