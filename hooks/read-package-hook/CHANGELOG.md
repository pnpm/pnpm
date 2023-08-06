# @pnpm/hooks.read-package-hook

## 3.0.5

### Patch Changes

- ec50dc98c: Compare overriding ranges with intersection instead of subset to fix override range bug [#6878](https://github.com/pnpm/pnpm/issues/6878).

## 3.0.4

### Patch Changes

- Updated dependencies [aa2ae8fe2]
  - @pnpm/types@9.2.0

## 3.0.3

### Patch Changes

- @pnpm/error@5.0.2
- @pnpm/parse-overrides@4.0.2

## 3.0.2

### Patch Changes

- Updated dependencies [a9e0b7cbf]
  - @pnpm/types@9.1.0
  - @pnpm/error@5.0.1
  - @pnpm/parse-overrides@4.0.1

## 3.0.1

### Patch Changes

- c0760128d: bump semver to 7.4.0

## 3.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- df107f2ef: Should use most specific override rule when multiple rules match the same target [#6210](https://github.com/pnpm/pnpm/issues/6210).
- 0a8b48f04: Update the compatibility DB.
- Updated dependencies [eceaa8b8b]
  - @pnpm/parse-wanted-dependency@5.0.0
  - @pnpm/parse-overrides@4.0.0
  - @pnpm/matcher@5.0.0
  - @pnpm/error@5.0.0
  - @pnpm/types@9.0.0

## 2.1.1

### Patch Changes

- d583fbb2a: Fixed issue where allowedVersions wouldn't apply correctly if only parent>child selectors were used

## 2.1.0

### Minor Changes

- f39d608ac: Extends the `pnpm.peerDependencyRules.allowedVersions` `package.json` option to support the `parent>child` selector syntax. This syntax allows for extending specific `peerDependencies` [#6108](https://github.com/pnpm/pnpm/pull/6108).

## 2.0.12

### Patch Changes

- 308eb2c9b: Use Map rather than Object in `createPackageExtender` to prevent read the prototype property to native function

## 2.0.11

### Patch Changes

- Updated dependencies [2ae1c449d]
  - @pnpm/parse-wanted-dependency@4.1.0
  - @pnpm/parse-overrides@3.0.3

## 2.0.10

### Patch Changes

- @pnpm/parse-overrides@3.0.2

## 2.0.9

### Patch Changes

- Updated dependencies [b77651d14]
  - @pnpm/types@8.10.0

## 2.0.8

### Patch Changes

- b11a8c363: It should be possible to use overrides with absolute file paths [#5754](https://github.com/pnpm/pnpm/issues/5754).

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
