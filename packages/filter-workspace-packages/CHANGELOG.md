# @pnpm/filter-workspace-packages

## 4.1.11

### Patch Changes

- @pnpm/find-workspace-packages@3.1.9

## 4.1.10

### Patch Changes

- @pnpm/find-workspace-packages@3.1.8

## 4.1.9

### Patch Changes

- @pnpm/find-workspace-packages@3.1.7

## 4.1.8

### Patch Changes

- @pnpm/find-workspace-packages@3.1.6

## 4.1.7

### Patch Changes

- @pnpm/find-workspace-packages@3.1.5

## 4.1.6

### Patch Changes

- @pnpm/find-workspace-packages@3.1.4

## 4.1.5

### Patch Changes

- @pnpm/find-workspace-packages@3.1.3

## 4.1.4

### Patch Changes

- @pnpm/find-workspace-packages@3.1.2

## 4.1.3

### Patch Changes

- @pnpm/find-workspace-packages@3.1.1

## 4.1.2

### Patch Changes

- Updated dependencies [a5bde0aa2]
  - @pnpm/find-workspace-packages@3.1.0

## 4.1.1

### Patch Changes

- @pnpm/find-workspace-packages@3.0.8

## 4.1.0

### Minor Changes

- c86fad004: New option added: `useGlobDirFiltering`. When `true`, directory filtering is done using globs.

## 4.0.6

### Patch Changes

- @pnpm/find-workspace-packages@3.0.7

## 4.0.5

### Patch Changes

- a1a03d145: Import only the required functions from ramda.
- Updated dependencies [a1a03d145]
  - pkgs-graph@6.1.2
  - @pnpm/find-workspace-packages@3.0.6

## 4.0.4

### Patch Changes

- @pnpm/find-workspace-packages@3.0.5

## 4.0.3

### Patch Changes

- @pnpm/find-workspace-packages@3.0.4

## 4.0.2

### Patch Changes

- @pnpm/find-workspace-packages@3.0.3

## 4.0.1

### Patch Changes

- Updated dependencies [1084ca1a7]
  - pkgs-graph@6.1.1

## 4.0.0

### Major Changes

- dfdf669e6: # @pnpm/filter-workspace-packages

  Change `@pnpm/filter-workspace-packages` to handle the new `filter-prod` flag, so that devDependencies are ignored if the filters / packageSelectors include `followProdDepsOnly` as true.

  ## filterPackages

  WHAT: Change `filterPackages`'s second arg to accept an array of objects with properties `filter` and `followProdDepsOnly`.

  WHY: Allow `filterPackages` to handle the filter-prod flag which allows the omission of devDependencies when building the package graph.

  HOW: Update your code by converting the filters into an array of objects. The `filter` property of this object maps to the filter that was previously passed in. The `followProdDepsOnly` is a boolean that will
  ignore devDependencies when building the package graph.

  If you do not care about ignoring devDependencies and want `filterPackages` to work as it did in the previous major version then you can use a simple map to convert your filters.

  ```
  const newFilters = oldFilters.map(filter => ({ filter, followProdDepsOnly: false }));
  ```

### Minor Changes

- dfdf669e6: Add new cli arg --filter-prod. --filter-prod acts the same as --filter, but it omits devDependencies when building dependencies

### Patch Changes

- Updated dependencies [dfdf669e6]
  - pkgs-graph@6.1.0
  - @pnpm/find-workspace-packages@3.0.2

## 3.0.1

### Patch Changes

- @pnpm/find-workspace-packages@3.0.1

## 3.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Patch Changes

- Updated dependencies [97b986fbc]
  - @pnpm/error@2.0.0
  - @pnpm/find-workspace-packages@3.0.0
  - @pnpm/matcher@2.0.0
  - pkgs-graph@6.0.0

## 2.3.14

### Patch Changes

- @pnpm/find-workspace-packages@2.3.42

## 2.3.13

### Patch Changes

- @pnpm/find-workspace-packages@2.3.41

## 2.3.12

### Patch Changes

- @pnpm/find-workspace-packages@2.3.40

## 2.3.11

### Patch Changes

- @pnpm/find-workspace-packages@2.3.39

## 2.3.10

### Patch Changes

- @pnpm/find-workspace-packages@2.3.38

## 2.3.9

### Patch Changes

- @pnpm/find-workspace-packages@2.3.37

## 2.3.8

### Patch Changes

- @pnpm/find-workspace-packages@2.3.36

## 2.3.7

### Patch Changes

- @pnpm/find-workspace-packages@2.3.35

## 2.3.6

### Patch Changes

- @pnpm/find-workspace-packages@2.3.34

## 2.3.5

### Patch Changes

- @pnpm/find-workspace-packages@2.3.33

## 2.3.4

### Patch Changes

- @pnpm/find-workspace-packages@2.3.32

## 2.3.3

### Patch Changes

- @pnpm/find-workspace-packages@2.3.31

## 2.3.2

### Patch Changes

- 32c9ef4be: execa updated to v5.
  - @pnpm/find-workspace-packages@2.3.30

## 2.3.1

### Patch Changes

- @pnpm/find-workspace-packages@2.3.29

## 2.3.0

### Minor Changes

- a8656b42f: New option added: `test-pattern`. `test-pattern` allows to detect whether the modified files are related to tests. If they are, the dependent packages of such modified packages are not included.

### Patch Changes

- @pnpm/find-workspace-packages@2.3.28

## 2.2.13

### Patch Changes

- @pnpm/find-workspace-packages@2.3.27

## 2.2.12

### Patch Changes

- 54ab5c87f: Dependencies of dependents should be included when using `...pkg...` filter.

## 2.2.11

### Patch Changes

- @pnpm/find-workspace-packages@2.3.26

## 2.2.10

### Patch Changes

- @pnpm/find-workspace-packages@2.3.25

## 2.2.9

### Patch Changes

- Updated dependencies [0c5f1bcc9]
  - @pnpm/error@1.4.0
  - @pnpm/find-workspace-packages@2.3.24

## 2.2.8

### Patch Changes

- @pnpm/find-workspace-packages@2.3.23

## 2.2.7

### Patch Changes

- @pnpm/find-workspace-packages@2.3.22

## 2.2.6

### Patch Changes

- @pnpm/find-workspace-packages@2.3.21

## 2.2.5

### Patch Changes

- @pnpm/find-workspace-packages@2.3.20

## 2.2.4

### Patch Changes

- @pnpm/find-workspace-packages@2.3.19

## 2.2.3

### Patch Changes

- @pnpm/find-workspace-packages@2.3.18

## 2.2.2

### Patch Changes

- @pnpm/find-workspace-packages@2.3.17

## 2.2.1

### Patch Changes

- @pnpm/find-workspace-packages@2.3.16

## 2.2.0

### Minor Changes

- a11aff299: If a package selector starts with "!", it will be excluded from the selection.

## 2.1.22

### Patch Changes

- @pnpm/find-workspace-packages@2.3.15

## 2.1.21

### Patch Changes

- @pnpm/find-workspace-packages@2.3.14

## 2.1.20

### Patch Changes

- @pnpm/find-workspace-packages@2.3.13

## 2.1.19

### Patch Changes

- @pnpm/find-workspace-packages@2.3.12

## 2.1.18

### Patch Changes

- Updated dependencies [75a36deba]
  - @pnpm/error@1.3.1
  - @pnpm/find-workspace-packages@2.3.11

## 2.1.17

### Patch Changes

- @pnpm/find-workspace-packages@2.3.10

## 2.1.16

### Patch Changes

- @pnpm/find-workspace-packages@2.3.9

## 2.1.15

### Patch Changes

- 999f81305: find-up updated to v5.
- Updated dependencies [6d480dd7a]
  - @pnpm/error@1.3.0
  - @pnpm/find-workspace-packages@2.3.8

## 2.1.14

### Patch Changes

- @pnpm/find-workspace-packages@2.3.7

## 2.1.13

### Patch Changes

- @pnpm/find-workspace-packages@2.3.6

## 2.1.12

### Patch Changes

- @pnpm/find-workspace-packages@2.3.5

## 2.1.11

### Patch Changes

- @pnpm/find-workspace-packages@2.3.4

## 2.1.10

### Patch Changes

- a2ef8084f: Use the same versions of dependencies across the pnpm monorepo.
- Updated dependencies [a2ef8084f]
  - @pnpm/find-workspace-packages@2.3.3

## 2.1.9

### Patch Changes

- @pnpm/find-workspace-packages@2.3.2

## 2.1.8

### Patch Changes

- @pnpm/find-workspace-packages@2.3.1

## 2.1.7

### Patch Changes

- Updated dependencies [faae9a93c]
  - @pnpm/find-workspace-packages@2.3.0

## 2.1.6

### Patch Changes

- @pnpm/find-workspace-packages@2.2.11

## 2.1.5

### Patch Changes

- @pnpm/find-workspace-packages@2.2.10

## 2.1.4

### Patch Changes

- @pnpm/find-workspace-packages@2.2.9

## 2.1.3

### Patch Changes

- @pnpm/find-workspace-packages@2.2.8

## 2.1.2

### Patch Changes

- @pnpm/find-workspace-packages@2.2.7

## 2.1.1

### Patch Changes

- Updated dependencies [71a8c8ce3]
  - @pnpm/matcher@1.0.3
  - @pnpm/find-workspace-packages@2.2.6

## 2.1.0

### Minor Changes

- e37a5a175: Support linkedWorkspacePackages=false.

### Patch Changes

- Updated dependencies [e37a5a175]
  - pkgs-graph@5.2.0

## 2.0.18

### Patch Changes

- @pnpm/find-workspace-packages@2.2.5

## 2.0.17

### Patch Changes

- @pnpm/find-workspace-packages@2.2.4

## 2.0.16

### Patch Changes

- @pnpm/find-workspace-packages@2.2.3

## 2.0.15

### Patch Changes

- @pnpm/error@1.2.1
- @pnpm/find-workspace-packages@2.2.2
- @pnpm/matcher@1.0.3
- pkgs-graph@5.1.6

## 2.0.15-alpha.2

### Patch Changes

- @pnpm/find-workspace-packages@2.2.2-alpha.2

## 2.0.15-alpha.1

### Patch Changes

- @pnpm/find-workspace-packages@2.2.2-alpha.1

## 2.0.15-alpha.0

### Patch Changes

- @pnpm/find-workspace-packages@2.2.2-alpha.0

## 2.0.14

### Patch Changes

- Updated dependencies [907c63a48]
  - @pnpm/matcher@1.0.2
  - @pnpm/find-workspace-packages@2.2.1
