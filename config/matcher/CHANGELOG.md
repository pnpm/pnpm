# @pnpm/matcher

## 4.0.1

### Patch Changes

- 969f8a002: The `pnpm.updateConfig.ignoreDependencies` setting should work with multiple dependencies in the array [#5639](https://github.com/pnpm/pnpm/issues/5639).

## 4.0.0

### Major Changes

- f884689e0: Require `@pnpm/logger` v5.

## 3.2.0

### Minor Changes

- abb41a626: New function added that returns the number of the matched pattern: matcherWithIndex().

## 3.1.0

### Minor Changes

- 9b44d38a4: Now it is possible to exclude packages from hoisting by prepending a `!` to the pattern. This works with both the `hoist-pattern` and `public-hoist-pattern` settings. For instance:

  ```
  public-hoist-pattern[]='*types*'
  public-hoist-pattern[]='!@types/react'

  hoist-pattern[]='*eslint*'
  hoist-pattern[]='!*eslint-plugin*'
  ```

  Ref [#5272](https://github.com/pnpm/pnpm/issues/5272)

## 3.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

## 2.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

## 1.0.3

### Patch Changes

- 71a8c8ce3: When no patterns are passed in, create a matcher that always returns `false`.

## 1.0.3

## 1.0.2

### Patch Changes

- 907c63a48: Dependencies updated.
