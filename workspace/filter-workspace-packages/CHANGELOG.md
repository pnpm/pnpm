# @pnpm/filter-workspace-packages

## 10.0.0

### Major Changes

- dd00eeb: Renamed dir to rootDir in the Project object.

### Patch Changes

- Updated dependencies [dd00eeb]
  - @pnpm/workspace.find-packages@4.0.0
  - @pnpm/workspace.pkgs-graph@4.0.0

## 9.0.0

### Major Changes

- Breaking changes to the API.

### Patch Changes

- Updated dependencies
  - @pnpm/workspace.find-packages@3.0.0
  - @pnpm/workspace.pkgs-graph@3.0.6

## 8.0.10

### Patch Changes

- @pnpm/workspace.find-packages@2.1.1

## 8.0.9

### Patch Changes

- Updated dependencies [b7ca13f]
  - @pnpm/workspace.find-packages@2.1.0

## 8.0.8

### Patch Changes

- @pnpm/workspace.pkgs-graph@3.0.5
- @pnpm/workspace.find-packages@2.0.7

## 8.0.7

### Patch Changes

- @pnpm/workspace.find-packages@2.0.6
- @pnpm/workspace.pkgs-graph@3.0.4

## 8.0.6

### Patch Changes

- Updated dependencies [a7aef51]
  - @pnpm/error@6.0.1
  - @pnpm/workspace.find-packages@2.0.5
  - @pnpm/workspace.pkgs-graph@3.0.3

## 8.0.5

### Patch Changes

- @pnpm/workspace.pkgs-graph@3.0.2
- @pnpm/workspace.find-packages@2.0.4

## 8.0.4

### Patch Changes

- @pnpm/workspace.pkgs-graph@3.0.1

## 8.0.3

### Patch Changes

- @pnpm/workspace.find-packages@2.0.3

## 8.0.2

### Patch Changes

- @pnpm/workspace.find-packages@2.0.2

## 8.0.1

### Patch Changes

- @pnpm/workspace.find-packages@2.0.1

## 8.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

### Patch Changes

- Updated dependencies [3ded840]
- Updated dependencies [43cdd87]
- Updated dependencies [ca2be03]
  - @pnpm/error@6.0.0
  - @pnpm/workspace.find-packages@2.0.0
  - @pnpm/workspace.pkgs-graph@3.0.0
  - @pnpm/matcher@6.0.0

## 7.2.11

### Patch Changes

- @pnpm/workspace.pkgs-graph@2.0.14
- @pnpm/workspace.find-packages@1.1.10

## 7.2.10

### Patch Changes

- @pnpm/workspace.find-packages@1.1.9

## 7.2.9

### Patch Changes

- @pnpm/workspace.find-packages@1.1.8

## 7.2.8

### Patch Changes

- @pnpm/workspace.pkgs-graph@2.0.13
- @pnpm/workspace.find-packages@1.1.7

## 7.2.7

### Patch Changes

- @pnpm/workspace.find-packages@1.1.6
- @pnpm/workspace.pkgs-graph@2.0.12

## 7.2.6

### Patch Changes

- @pnpm/workspace.find-packages@1.1.5

## 7.2.5

### Patch Changes

- @pnpm/workspace.find-packages@1.1.4

## 7.2.4

### Patch Changes

- @pnpm/workspace.pkgs-graph@2.0.11

## 7.2.3

### Patch Changes

- @pnpm/workspace.find-packages@1.1.3

## 7.2.2

### Patch Changes

- @pnpm/workspace.find-packages@1.1.2

## 7.2.1

### Patch Changes

- @pnpm/workspace.pkgs-graph@2.0.10
- @pnpm/workspace.find-packages@1.1.1

## 7.2.0

### Minor Changes

- 43ce9e4a6: Support for multiple architectures when installing dependencies [#5965](https://github.com/pnpm/pnpm/issues/5965).

  You can now specify architectures for which you'd like to install optional dependencies, even if they don't match the architecture of the system running the install. Use the `supportedArchitectures` field in `package.json` to define your preferences.

  For example, the following configuration tells pnpm to install optional dependencies for Windows x64:

  ```json
  {
    "pnpm": {
      "supportedArchitectures": {
        "os": ["win32"],
        "cpu": ["x64"]
      }
    }
  }
  ```

  Whereas this configuration will have pnpm install optional dependencies for Windows, macOS, and the architecture of the system currently running the install. It includes artifacts for both x64 and arm64 CPUs:

  ```json
  {
    "pnpm": {
      "supportedArchitectures": {
        "os": ["win32", "darwin", "current"],
        "cpu": ["x64", "arm64"]
      }
    }
  }
  ```

  Additionally, `supportedArchitectures` also supports specifying the `libc` of the system.

### Patch Changes

- Updated dependencies [43ce9e4a6]
  - @pnpm/workspace.find-packages@1.1.0
  - @pnpm/workspace.pkgs-graph@2.0.9

## 7.1.4

### Patch Changes

- @pnpm/workspace.pkgs-graph@2.0.8
- @pnpm/workspace.find-packages@1.0.14

## 7.1.3

### Patch Changes

- @pnpm/workspace.find-packages@1.0.13

## 7.1.2

### Patch Changes

- @pnpm/workspace.find-packages@1.0.12
- @pnpm/workspace.pkgs-graph@2.0.7

## 7.1.1

### Patch Changes

- @pnpm/workspace.find-packages@1.0.11

## 7.1.0

### Minor Changes

- a6f5e5c9c: Fix a bug in which `use-node-version` or `node-version` isn't passed down to `checkEngine` when using pnpm workspace, resulting in an error [#6981](https://github.com/pnpm/pnpm/issues/6981).

### Patch Changes

- @pnpm/workspace.find-packages@1.0.10

## 7.0.19

### Patch Changes

- @pnpm/workspace.pkgs-graph@2.0.6
- @pnpm/workspace.find-packages@1.0.9

## 7.0.18

### Patch Changes

- @pnpm/workspace.find-packages@1.0.8

## 7.0.17

### Patch Changes

- @pnpm/workspace.find-packages@1.0.7

## 7.0.16

### Patch Changes

- Updated dependencies [41c2b65cf]
  - @pnpm/workspace.pkgs-graph@2.0.5
  - @pnpm/workspace.find-packages@1.0.6

## 7.0.15

### Patch Changes

- 12b0f0976: `pnpm patch` should works correctly when shared-workspace-file is false [#6885](https://github.com/pnpm/pnpm/issues/6885)
- Updated dependencies [12b0f0976]
  - @pnpm/workspace.find-packages@1.0.5

## 7.0.14

### Patch Changes

- @pnpm/workspace.find-packages@1.0.4

## 7.0.13

### Patch Changes

- @pnpm/workspace.find-packages@1.0.3

## 7.0.12

### Patch Changes

- @pnpm/workspace.find-packages@1.0.2

## 7.0.11

### Patch Changes

- @pnpm/workspace.find-packages@1.0.1

## 7.0.10

### Patch Changes

- Updated dependencies [bd235794d]
- Updated dependencies [bd235794d]
  - @pnpm/workspace.find-packages@1.0.0
  - @pnpm/error@5.0.2

## 7.0.9

### Patch Changes

- @pnpm/find-workspace-packages@6.0.9
- @pnpm/error@5.0.1

## 7.0.8

### Patch Changes

- @pnpm/find-workspace-packages@6.0.8

## 7.0.7

### Patch Changes

- @pnpm/find-workspace-packages@6.0.7

## 7.0.6

### Patch Changes

- @pnpm/workspace.pkgs-graph@2.0.4
- @pnpm/find-workspace-packages@6.0.6

## 7.0.5

### Patch Changes

- @pnpm/find-workspace-packages@6.0.5

## 7.0.4

### Patch Changes

- @pnpm/find-workspace-packages@6.0.4

## 7.0.3

### Patch Changes

- Updated dependencies [9fd0e375e]
  - @pnpm/workspace.pkgs-graph@2.0.3
  - @pnpm/find-workspace-packages@6.0.3

## 7.0.2

### Patch Changes

- Updated dependencies [35d98c7a8]
  - @pnpm/workspace.pkgs-graph@2.0.2
  - @pnpm/find-workspace-packages@6.0.2

## 7.0.1

### Patch Changes

- Updated dependencies [572068180]
  - @pnpm/workspace.pkgs-graph@2.0.1
  - @pnpm/find-workspace-packages@6.0.1

## 7.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [eceaa8b8b]
  - @pnpm/find-workspace-packages@6.0.0
  - @pnpm/workspace.pkgs-graph@2.0.0
  - @pnpm/matcher@5.0.0
  - @pnpm/error@5.0.0

## 6.0.42

### Patch Changes

- @pnpm/find-workspace-packages@5.0.42

## 6.0.41

### Patch Changes

- @pnpm/find-workspace-packages@5.0.41

## 6.0.40

### Patch Changes

- @pnpm/find-workspace-packages@5.0.40

## 6.0.39

### Patch Changes

- @pnpm/find-workspace-packages@5.0.39

## 6.0.38

### Patch Changes

- @pnpm/find-workspace-packages@5.0.38

## 6.0.37

### Patch Changes

- @pnpm/find-workspace-packages@5.0.37

## 6.0.36

### Patch Changes

- @pnpm/find-workspace-packages@5.0.36

## 6.0.35

### Patch Changes

- @pnpm/find-workspace-packages@5.0.35

## 6.0.34

### Patch Changes

- @pnpm/find-workspace-packages@5.0.34

## 6.0.33

### Patch Changes

- @pnpm/find-workspace-packages@5.0.33

## 6.0.32

### Patch Changes

- @pnpm/find-workspace-packages@5.0.32

## 6.0.31

### Patch Changes

- @pnpm/find-workspace-packages@5.0.31

## 6.0.30

### Patch Changes

- @pnpm/find-workspace-packages@5.0.30

## 6.0.29

### Patch Changes

- @pnpm/find-workspace-packages@5.0.29

## 6.0.28

### Patch Changes

- @pnpm/find-workspace-packages@5.0.28

## 6.0.27

### Patch Changes

- @pnpm/find-workspace-packages@5.0.27

## 6.0.26

### Patch Changes

- @pnpm/find-workspace-packages@5.0.26

## 6.0.25

### Patch Changes

- @pnpm/find-workspace-packages@5.0.25

## 6.0.24

### Patch Changes

- @pnpm/find-workspace-packages@5.0.24

## 6.0.23

### Patch Changes

- @pnpm/find-workspace-packages@5.0.23

## 6.0.22

### Patch Changes

- @pnpm/error@4.0.1
- @pnpm/find-workspace-packages@5.0.22

## 6.0.21

### Patch Changes

- @pnpm/find-workspace-packages@5.0.21

## 6.0.20

### Patch Changes

- 08ceaf3fc: replace dependency `is-ci` by `ci-info` (`is-ci` is just a simple wrapper around `ci-info`).
  - @pnpm/find-workspace-packages@5.0.20

## 6.0.19

### Patch Changes

- @pnpm/find-workspace-packages@5.0.19

## 6.0.18

### Patch Changes

- @pnpm/find-workspace-packages@5.0.18

## 6.0.17

### Patch Changes

- @pnpm/find-workspace-packages@5.0.17

## 6.0.16

### Patch Changes

- Updated dependencies [313702d76]
  - @pnpm/workspace.pkgs-graph@1.0.0
  - @pnpm/find-workspace-packages@5.0.16

## 6.0.15

### Patch Changes

- @pnpm/find-workspace-packages@5.0.15

## 6.0.14

### Patch Changes

- @pnpm/find-workspace-packages@5.0.14

## 6.0.13

### Patch Changes

- @pnpm/find-workspace-packages@5.0.13

## 6.0.12

### Patch Changes

- @pnpm/find-workspace-packages@5.0.12

## 6.0.11

### Patch Changes

- @pnpm/find-workspace-packages@5.0.11

## 6.0.10

### Patch Changes

- @pnpm/find-workspace-packages@5.0.10

## 6.0.9

### Patch Changes

- Updated dependencies [969f8a002]
  - @pnpm/matcher@4.0.1
  - @pnpm/find-workspace-packages@5.0.9

## 6.0.8

### Patch Changes

- @pnpm/find-workspace-packages@5.0.8

## 6.0.7

### Patch Changes

- @pnpm/find-workspace-packages@5.0.7

## 6.0.6

### Patch Changes

- @pnpm/find-workspace-packages@5.0.6

## 6.0.5

### Patch Changes

- Updated dependencies [2e9790722]
  - @pnpm/find-workspace-packages@5.0.5

## 6.0.4

### Patch Changes

- @pnpm/find-workspace-packages@5.0.4

## 6.0.3

### Patch Changes

- @pnpm/find-workspace-packages@5.0.3

## 6.0.2

### Patch Changes

- @pnpm/find-workspace-packages@5.0.2

## 6.0.1

### Patch Changes

- @pnpm/find-workspace-packages@5.0.1

## 6.0.0

### Major Changes

- 043d988fc: Breaking change to the API. Defaul export is not used.
- f884689e0: Require `@pnpm/logger` v5.

### Minor Changes

- 645384bfd: `readProjects()` returns `allProjectsGraph`.

### Patch Changes

- Updated dependencies [043d988fc]
- Updated dependencies [f884689e0]
  - @pnpm/error@4.0.0
  - @pnpm/find-workspace-packages@5.0.0
  - @pnpm/matcher@4.0.0
  - pkgs-graph@8.0.0

## 5.1.3

### Patch Changes

- @pnpm/find-workspace-packages@4.0.43

## 5.1.2

### Patch Changes

- @pnpm/find-workspace-packages@4.0.42

## 5.1.1

### Patch Changes

- Updated dependencies [e8a631bf0]
  - @pnpm/error@3.1.0
  - @pnpm/find-workspace-packages@4.0.41

## 5.1.0

### Minor Changes

- 2e830c0cb: New function exported: `filterPackagesFromDir`.

### Patch Changes

- Updated dependencies [abb41a626]
  - @pnpm/matcher@3.2.0
  - @pnpm/find-workspace-packages@4.0.40

## 5.0.39

### Patch Changes

- @pnpm/find-workspace-packages@4.0.39

## 5.0.38

### Patch Changes

- @pnpm/find-workspace-packages@4.0.38

## 5.0.37

### Patch Changes

- Updated dependencies [9b44d38a4]
  - @pnpm/matcher@3.1.0
  - @pnpm/find-workspace-packages@4.0.37

## 5.0.36

### Patch Changes

- @pnpm/find-workspace-packages@4.0.36

## 5.0.35

### Patch Changes

- @pnpm/find-workspace-packages@4.0.35

## 5.0.34

### Patch Changes

- @pnpm/find-workspace-packages@4.0.34

## 5.0.33

### Patch Changes

- @pnpm/find-workspace-packages@4.0.33

## 5.0.32

### Patch Changes

- @pnpm/find-workspace-packages@4.0.32

## 5.0.31

### Patch Changes

- @pnpm/find-workspace-packages@4.0.31

## 5.0.30

### Patch Changes

- @pnpm/find-workspace-packages@4.0.30

## 5.0.29

### Patch Changes

- @pnpm/find-workspace-packages@4.0.29

## 5.0.28

### Patch Changes

- @pnpm/find-workspace-packages@4.0.28

## 5.0.27

### Patch Changes

- @pnpm/find-workspace-packages@4.0.27

## 5.0.26

### Patch Changes

- 8103f92bd: Use a patched version of ramda to fix deprecation warnings on Node.js 16. Related issue: https://github.com/ramda/ramda/pull/3270
- Updated dependencies [8103f92bd]
  - pkgs-graph@7.0.2
  - @pnpm/find-workspace-packages@4.0.26

## 5.0.25

### Patch Changes

- @pnpm/find-workspace-packages@4.0.25

## 5.0.24

### Patch Changes

- @pnpm/find-workspace-packages@4.0.24

## 5.0.23

### Patch Changes

- @pnpm/find-workspace-packages@4.0.23

## 5.0.22

### Patch Changes

- @pnpm/find-workspace-packages@4.0.22

## 5.0.21

### Patch Changes

- @pnpm/find-workspace-packages@4.0.21

## 5.0.20

### Patch Changes

- @pnpm/find-workspace-packages@4.0.20

## 5.0.19

### Patch Changes

- @pnpm/find-workspace-packages@4.0.19

## 5.0.18

### Patch Changes

- @pnpm/find-workspace-packages@4.0.18

## 5.0.17

### Patch Changes

- 5f643f23b: Update ramda to v0.28.
- Updated dependencies [5f643f23b]
- Updated dependencies [42c1ea1c0]
  - pkgs-graph@7.0.1
  - @pnpm/find-workspace-packages@4.0.17

## 5.0.16

### Patch Changes

- @pnpm/find-workspace-packages@4.0.16

## 5.0.15

### Patch Changes

- @pnpm/find-workspace-packages@4.0.15

## 5.0.14

### Patch Changes

- @pnpm/find-workspace-packages@4.0.14

## 5.0.13

### Patch Changes

- @pnpm/find-workspace-packages@4.0.13

## 5.0.12

### Patch Changes

- @pnpm/find-workspace-packages@4.0.12

## 5.0.11

### Patch Changes

- @pnpm/find-workspace-packages@4.0.11

## 5.0.10

### Patch Changes

- @pnpm/find-workspace-packages@4.0.10

## 5.0.9

### Patch Changes

- @pnpm/find-workspace-packages@4.0.9

## 5.0.8

### Patch Changes

- @pnpm/find-workspace-packages@4.0.8

## 5.0.7

### Patch Changes

- @pnpm/find-workspace-packages@4.0.7

## 5.0.6

### Patch Changes

- @pnpm/find-workspace-packages@4.0.6

## 5.0.5

### Patch Changes

- @pnpm/find-workspace-packages@4.0.5

## 5.0.4

### Patch Changes

- @pnpm/find-workspace-packages@4.0.4

## 5.0.3

### Patch Changes

- @pnpm/find-workspace-packages@4.0.3

## 5.0.2

### Patch Changes

- 9f0616282: 'filter-workspace-packages' will filter the package well even if Korean is included in the path. fix #4594
  - @pnpm/find-workspace-packages@4.0.2

## 5.0.1

### Patch Changes

- @pnpm/error@3.0.1
- @pnpm/find-workspace-packages@4.0.1

## 5.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [542014839]
  - @pnpm/error@3.0.0
  - @pnpm/find-workspace-packages@4.0.0
  - @pnpm/matcher@3.0.0
  - pkgs-graph@7.0.0

## 4.4.22

### Patch Changes

- Updated dependencies [70ba51da9]
  - @pnpm/error@2.1.0
  - @pnpm/find-workspace-packages@3.1.42

## 4.4.21

### Patch Changes

- @pnpm/find-workspace-packages@3.1.41

## 4.4.20

### Patch Changes

- @pnpm/find-workspace-packages@3.1.40

## 4.4.19

### Patch Changes

- @pnpm/find-workspace-packages@3.1.39

## 4.4.18

### Patch Changes

- @pnpm/find-workspace-packages@3.1.38

## 4.4.17

### Patch Changes

- @pnpm/find-workspace-packages@3.1.37

## 4.4.16

### Patch Changes

- @pnpm/find-workspace-packages@3.1.36

## 4.4.15

### Patch Changes

- @pnpm/find-workspace-packages@3.1.35

## 4.4.14

### Patch Changes

- @pnpm/find-workspace-packages@3.1.34

## 4.4.13

### Patch Changes

- @pnpm/find-workspace-packages@3.1.33

## 4.4.12

### Patch Changes

- @pnpm/find-workspace-packages@3.1.32

## 4.4.11

### Patch Changes

- @pnpm/find-workspace-packages@3.1.31

## 4.4.10

### Patch Changes

- @pnpm/find-workspace-packages@3.1.30

## 4.4.9

### Patch Changes

- Updated dependencies [f82cc7f77]
  - pkgs-graph@6.1.3
  - @pnpm/find-workspace-packages@3.1.29

## 4.4.8

### Patch Changes

- @pnpm/find-workspace-packages@3.1.28

## 4.4.7

### Patch Changes

- @pnpm/find-workspace-packages@3.1.27

## 4.4.6

### Patch Changes

- @pnpm/find-workspace-packages@3.1.26

## 4.4.5

### Patch Changes

- @pnpm/find-workspace-packages@3.1.25

## 4.4.4

### Patch Changes

- @pnpm/find-workspace-packages@3.1.24

## 4.4.3

### Patch Changes

- @pnpm/find-workspace-packages@3.1.23

## 4.4.2

### Patch Changes

- @pnpm/find-workspace-packages@3.1.22

## 4.4.1

### Patch Changes

- @pnpm/find-workspace-packages@3.1.21

## 4.4.0

### Minor Changes

- 456232654: Make the scope of the package optional, when filtering.

### Patch Changes

- @pnpm/find-workspace-packages@3.1.20

## 4.3.3

### Patch Changes

- @pnpm/find-workspace-packages@3.1.19

## 4.3.2

### Patch Changes

- @pnpm/find-workspace-packages@3.1.18

## 4.3.1

### Patch Changes

- @pnpm/find-workspace-packages@3.1.17

## 4.3.0

### Minor Changes

- dcc9cb746: New optional option added to `readProjects()`: engineStrict.

## 4.2.1

### Patch Changes

- @pnpm/find-workspace-packages@3.1.16

## 4.2.0

### Minor Changes

- fe5688dc0: Add option 'changed-files-ignore-pattern' to ignore changed files by glob patterns when filtering for changed projects since the specified commit/branch.

### Patch Changes

- @pnpm/find-workspace-packages@3.1.15

## 4.1.17

### Patch Changes

- @pnpm/find-workspace-packages@3.1.14

## 4.1.16

### Patch Changes

- 04b7f6086: Use safe-execa instead of execa to prevent binary planting attacks on Windows.

## 4.1.15

### Patch Changes

- @pnpm/find-workspace-packages@3.1.13

## 4.1.14

### Patch Changes

- @pnpm/find-workspace-packages@3.1.12

## 4.1.13

### Patch Changes

- @pnpm/find-workspace-packages@3.1.11

## 4.1.12

### Patch Changes

- @pnpm/find-workspace-packages@3.1.10

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
