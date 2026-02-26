# @pnpm/tools.plugin-commands-self-updater

## 1000.1.51

### Patch Changes

- 1f7425b: Fixed version switching via `packageManager` field failing when pnpm is installed as a standalone executable in environments without a system Node.js [#10687](https://github.com/pnpm/pnpm/issues/10687).

## 1000.1.50

### Patch Changes

- Updated dependencies [dac1177]
  - @pnpm/link-bins@1000.3.7
  - @pnpm/client@1001.1.21
  - @pnpm/cli-utils@1001.3.7

## 1000.1.49

### Patch Changes

- @pnpm/read-project-manifest@1001.2.5
- @pnpm/client@1001.1.20
- @pnpm/cli-utils@1001.3.6
- @pnpm/config@1004.10.2
- @pnpm/link-bins@1000.3.6

## 1000.1.48

### Patch Changes

- @pnpm/cli-utils@1001.3.5

## 1000.1.47

### Patch Changes

- Updated dependencies [00c7677]
  - @pnpm/cli-utils@1001.3.4

## 1000.1.46

### Patch Changes

- Updated dependencies [595cd41]
  - @pnpm/config@1004.10.1
  - @pnpm/cli-utils@1001.3.3
  - @pnpm/client@1001.1.19

## 1000.1.45

### Patch Changes

- Updated dependencies [7d8be9f]
- Updated dependencies [7f18264]
- Updated dependencies [a57ba4e]
  - @pnpm/config@1004.10.0
  - @pnpm/cli-utils@1001.3.2
  - @pnpm/client@1001.1.18

## 1000.1.44

### Patch Changes

- @pnpm/link-bins@1000.3.5
- @pnpm/client@1001.1.17
- @pnpm/cli-utils@1001.3.1
- @pnpm/config@1004.9.2

## 1000.1.43

### Patch Changes

- Updated dependencies [d75628a]
  - @pnpm/cli-utils@1001.3.0
  - @pnpm/cli-meta@1000.0.16
  - @pnpm/config@1004.9.2
  - @pnpm/client@1001.1.16
  - @pnpm/link-bins@1000.3.4
  - @pnpm/read-project-manifest@1001.2.4

## 1000.1.42

### Patch Changes

- Updated dependencies [f022a1b]
  - @pnpm/config@1004.9.1
  - @pnpm/cli-utils@1001.2.19
  - @pnpm/client@1001.1.15

## 1000.1.41

### Patch Changes

- Updated dependencies [3f2c5f4]
- Updated dependencies [99e1ada]
  - @pnpm/config@1004.9.0
  - @pnpm/client@1001.1.14
  - @pnpm/cli-utils@1001.2.18

## 1000.1.40

### Patch Changes

- @pnpm/client@1001.1.13
- @pnpm/cli-utils@1001.2.17
- @pnpm/config@1004.8.1

## 1000.1.39

### Patch Changes

- @pnpm/client@1001.1.12
- @pnpm/cli-utils@1001.2.16

## 1000.1.38

### Patch Changes

- Updated dependencies [73cc635]
- Updated dependencies [59a81aa]
  - @pnpm/config@1004.8.0
  - @pnpm/cli-utils@1001.2.15
  - @pnpm/cli-meta@1000.0.15
  - @pnpm/client@1001.1.11
  - @pnpm/link-bins@1000.3.3
  - @pnpm/read-project-manifest@1001.2.3

## 1000.1.37

### Patch Changes

- @pnpm/cli-utils@1001.2.14
- @pnpm/cli-meta@1000.0.14
- @pnpm/config@1004.7.1
- @pnpm/client@1001.1.10
- @pnpm/link-bins@1000.3.2
- @pnpm/read-project-manifest@1001.2.2

## 1000.1.36

### Patch Changes

- Updated dependencies [b0ec709]
  - @pnpm/config@1004.7.0
  - @pnpm/cli-utils@1001.2.13
  - @pnpm/client@1001.1.9

## 1000.1.35

### Patch Changes

- Reverted: `pnpm self-update` should download pnpm from the configured npm registry [#10205](https://github.com/pnpm/pnpm/pull/10205).
- Updated dependencies [615c066]
  - @pnpm/config@1004.6.2
  - @pnpm/cli-utils@1001.2.12
  - @pnpm/client@1001.1.8

## 1000.1.34

### Patch Changes

- 2194432: `pnpm self-update` should download pnpm from the configured npm registry [#10205](https://github.com/pnpm/pnpm/pull/10205).
- e75aaed: `pnpm self-update` should always install the non-executable pnpm package (pnpm in the registry) and never the `@pnpm/exe` package, when installing v11 or newer. We currently cannot ship `@pnpm/exe` as `pkg` doesn't work with ESM [#10190](https://github.com/pnpm/pnpm/pull/10190).
- Updated dependencies [2fc23e4]
  - @pnpm/read-project-manifest@1001.2.1
  - @pnpm/cli-utils@1001.2.11
  - @pnpm/config@1004.6.1
  - @pnpm/link-bins@1000.3.1
  - @pnpm/cli-meta@1000.0.13
  - @pnpm/client@1001.1.7

## 1000.1.33

### Patch Changes

- Updated dependencies [93d4954]
  - @pnpm/config@1004.6.0
  - @pnpm/client@1001.1.6
  - @pnpm/cli-utils@1001.2.10

## 1000.1.32

### Patch Changes

- Updated dependencies [7b19077]
- Updated dependencies [68ad086]
- Updated dependencies [5847af4]
- Updated dependencies [36eb104]
  - @pnpm/config@1004.5.0
  - @pnpm/read-project-manifest@1001.2.0
  - @pnpm/link-bins@1000.3.0
  - @pnpm/cli-utils@1001.2.9
  - @pnpm/cli-meta@1000.0.12
  - @pnpm/client@1001.1.5

## 1000.1.31

### Patch Changes

- @pnpm/cli-utils@1001.2.8
- @pnpm/client@1001.1.4

## 1000.1.30

### Patch Changes

- @pnpm/cli-meta@1000.0.11
- @pnpm/cli-utils@1001.2.7
- @pnpm/config@1004.4.2
- @pnpm/client@1001.1.3
- @pnpm/link-bins@1000.2.6
- @pnpm/read-project-manifest@1001.1.4

## 1000.1.29

### Patch Changes

- Updated dependencies [9865167]
- Updated dependencies [a8797c4]
  - @pnpm/config@1004.4.1
  - @pnpm/link-bins@1000.2.5
  - @pnpm/cli-utils@1001.2.6
  - @pnpm/client@1001.1.2

## 1000.1.28

### Patch Changes

- 3d9a3c8: pnpm version switching should work when the pnpm home directory is in a symlinked directory [#9715](https://github.com/pnpm/pnpm/issues/9715).
  - @pnpm/client@1001.1.1
  - @pnpm/cli-utils@1001.2.5

## 1000.1.27

### Patch Changes

- Updated dependencies [fb4da0c]
  - @pnpm/client@1001.1.0
  - @pnpm/config@1004.4.0
  - @pnpm/read-project-manifest@1001.1.3
  - @pnpm/cli-utils@1001.2.4
  - @pnpm/link-bins@1000.2.4

## 1000.1.26

### Patch Changes

- @pnpm/cli-utils@1001.2.3
- @pnpm/client@1001.0.7

## 1000.1.25

### Patch Changes

- @pnpm/cli-utils@1001.2.2
- @pnpm/client@1001.0.6

## 1000.1.24

### Patch Changes

- @pnpm/config@1004.3.1
- @pnpm/error@1000.0.5
- @pnpm/link-bins@1000.2.3
- @pnpm/cli-utils@1001.2.1
- @pnpm/read-project-manifest@1001.1.2
- @pnpm/client@1001.0.5

## 1000.1.23

### Patch Changes

- Updated dependencies [38e2599]
- Updated dependencies [e792927]
  - @pnpm/config@1004.3.0
  - @pnpm/cli-utils@1001.2.0
  - @pnpm/link-bins@1000.2.2
  - @pnpm/cli-meta@1000.0.10
  - @pnpm/client@1001.0.4
  - @pnpm/read-project-manifest@1001.1.1

## 1000.1.22

### Patch Changes

- Updated dependencies [affdd5b]
  - @pnpm/link-bins@1000.2.1
  - @pnpm/client@1001.0.3
  - @pnpm/cli-utils@1001.1.2

## 1000.1.21

### Patch Changes

- @pnpm/client@1001.0.2
- @pnpm/cli-utils@1001.1.1

## 1000.1.20

### Patch Changes

- Updated dependencies [3ebc0ce]
  - @pnpm/cli-utils@1001.1.0
  - @pnpm/client@1001.0.1

## 1000.1.19

### Patch Changes

- Updated dependencies [d1edf73]
- Updated dependencies [d1edf73]
- Updated dependencies [86b33e9]
- Updated dependencies [d1edf73]
- Updated dependencies [f91922c]
  - @pnpm/link-bins@1000.2.0
  - @pnpm/read-project-manifest@1001.1.0
  - @pnpm/client@1001.0.0
  - @pnpm/config@1004.2.1
  - @pnpm/error@1000.0.4
  - @pnpm/cli-utils@1001.0.3

## 1000.1.18

### Patch Changes

- Updated dependencies [1a07b8f]
- Updated dependencies [1a07b8f]
- Updated dependencies [6f7ac0f]
- Updated dependencies [1a07b8f]
  - @pnpm/link-bins@1000.1.0
  - @pnpm/read-project-manifest@1001.0.0
  - @pnpm/config@1004.2.0
  - @pnpm/client@1000.1.0
  - @pnpm/cli-meta@1000.0.9
  - @pnpm/cli-utils@1001.0.2
  - @pnpm/error@1000.0.3

## 1000.1.17

### Patch Changes

- Updated dependencies [7ad0bc3]
  - @pnpm/cli-utils@1001.0.1

## 1000.1.16

### Patch Changes

- Updated dependencies [623da6f]
- Updated dependencies [cf630a8]
- Updated dependencies [e225310]
  - @pnpm/config@1004.1.0
  - @pnpm/cli-utils@1001.0.0
  - @pnpm/client@1000.0.21

## 1000.1.15

### Patch Changes

- @pnpm/cli-utils@1000.1.7

## 1000.1.14

### Patch Changes

- Updated dependencies [b217bbb]
- Updated dependencies [b0ead51]
- Updated dependencies [c8341cc]
- Updated dependencies [b0ead51]
- Updated dependencies [046af72]
  - @pnpm/config@1004.0.0
  - @pnpm/client@1000.0.20
  - @pnpm/cli-utils@1000.1.6

## 1000.1.13

### Patch Changes

- Updated dependencies [8d175c0]
  - @pnpm/config@1003.1.1
  - @pnpm/cli-utils@1000.1.5
  - @pnpm/client@1000.0.19

## 1000.1.12

### Patch Changes

- df8df8a: pnpm version management should work, when `dangerouslyAllowAllBuilds` is set to `true` [#9472](https://github.com/pnpm/pnpm/issues/9472).
- 09cf46f: Update `@pnpm/logger` in peer dependencies.
- Updated dependencies [b282bd1]
- Updated dependencies [fdb1d98]
- Updated dependencies [e4af08c]
- Updated dependencies [09cf46f]
- Updated dependencies [36d1448]
- Updated dependencies [9362b5f]
- Updated dependencies [6cf010c]
  - @pnpm/config@1003.1.0
  - @pnpm/link-bins@1000.0.13
  - @pnpm/cli-utils@1000.1.4
  - @pnpm/client@1000.0.18
  - @pnpm/cli-meta@1000.0.8
  - @pnpm/read-project-manifest@1000.0.11

## 1000.1.11

### Patch Changes

- Updated dependencies [fa1e69b]
  - @pnpm/link-bins@1000.0.12
  - @pnpm/cli-utils@1000.1.3
  - @pnpm/config@1003.0.1
  - @pnpm/client@1000.0.17

## 1000.1.10

### Patch Changes

- Updated dependencies [56bb69b]
- Updated dependencies [8a9f3a4]
- Updated dependencies [9c3dd03]
  - @pnpm/config@1003.0.0
  - @pnpm/logger@1001.0.0
  - @pnpm/cli-utils@1000.1.2
  - @pnpm/client@1000.0.16
  - @pnpm/link-bins@1000.0.11
  - @pnpm/cli-meta@1000.0.7
  - @pnpm/read-project-manifest@1000.0.10

## 1000.1.9

### Patch Changes

- @pnpm/client@1000.0.15
- @pnpm/cli-utils@1000.1.1
- @pnpm/config@1002.7.2

## 1000.1.8

### Patch Changes

- Updated dependencies [5679712]
- Updated dependencies [01f2bcf]
- Updated dependencies [1413c25]
  - @pnpm/config@1002.7.1
  - @pnpm/cli-utils@1000.1.0
  - @pnpm/cli-meta@1000.0.6
  - @pnpm/client@1000.0.14
  - @pnpm/link-bins@1000.0.10
  - @pnpm/read-project-manifest@1000.0.9

## 1000.1.7

### Patch Changes

- Updated dependencies [e57f1df]
  - @pnpm/config@1002.7.0
  - @pnpm/cli-utils@1000.0.19

## 1000.1.6

### Patch Changes

- Updated dependencies [9bcca9f]
- Updated dependencies [5b35dff]
- Updated dependencies [9bcca9f]
- Updated dependencies [5f7be64]
- Updated dependencies [5f7be64]
  - @pnpm/config@1002.6.0
  - @pnpm/cli-utils@1000.0.18
  - @pnpm/cli-meta@1000.0.5
  - @pnpm/pick-registry-for-package@1000.0.5
  - @pnpm/client@1000.0.13
  - @pnpm/link-bins@1000.0.9
  - @pnpm/read-project-manifest@1000.0.8

## 1000.1.5

### Patch Changes

- Updated dependencies [936430a]
  - @pnpm/config@1002.5.4
  - @pnpm/cli-utils@1000.0.17
  - @pnpm/client@1000.0.12

## 1000.1.4

### Patch Changes

- @pnpm/client@1000.0.11

## 1000.1.3

### Patch Changes

- Updated dependencies [6e4459c]
  - @pnpm/config@1002.5.3
  - @pnpm/cli-utils@1000.0.16

## 1000.1.2

### Patch Changes

- 7072838: `pnpm self-update` should always update the version in the `packageManager` field of `package.json`.
- Updated dependencies [0b0bcfa]
  - @pnpm/exec.pnpm-cli-runner@1000.0.1
  - @pnpm/cli-utils@1000.0.15
  - @pnpm/config@1002.5.2
  - @pnpm/client@1000.0.10

## 1000.1.1

### Patch Changes

- Updated dependencies [c3aa4d8]
  - @pnpm/config@1002.5.1
  - @pnpm/cli-utils@1000.0.14
  - @pnpm/client@1000.0.9

## 1000.1.0

### Minor Changes

- 6a59366: Export `installPnpmToTools`.

### Patch Changes

- e091871: `pnpm self-update` should not leave a directory with a broken pnpm installation if the installation fails.
- 6a59366: `pnpm self-update` should not read the pnpm settings from the `package.json` file in the current working directory.
- Updated dependencies [d965748]
  - @pnpm/config@1002.5.0
  - @pnpm/link-bins@1000.0.8
  - @pnpm/cli-meta@1000.0.4
  - @pnpm/cli-utils@1000.0.13
  - @pnpm/pick-registry-for-package@1000.0.4
  - @pnpm/client@1000.0.8
  - @pnpm/read-project-manifest@1000.0.7

## 1000.0.14

### Patch Changes

- Updated dependencies [76973d8]
- Updated dependencies [1c2eb8c]
  - @pnpm/plugin-commands-installation@1002.0.1
  - @pnpm/config@1002.4.1
  - @pnpm/cli-utils@1000.0.12

## 1000.0.13

### Patch Changes

- Updated dependencies [8fcc221]
- Updated dependencies [8fcc221]
- Updated dependencies [e32b1a2]
- Updated dependencies [5296961]
  - @pnpm/plugin-commands-installation@1002.0.0
  - @pnpm/config@1002.4.0
  - @pnpm/cli-utils@1000.0.11
  - @pnpm/cli-meta@1000.0.3
  - @pnpm/pick-registry-for-package@1000.0.3
  - @pnpm/client@1000.0.7
  - @pnpm/link-bins@1000.0.7
  - @pnpm/read-project-manifest@1000.0.6

## 1000.0.12

### Patch Changes

- Updated dependencies [fee898f]
- Updated dependencies [546ab37]
  - @pnpm/config@1002.3.1
  - @pnpm/plugin-commands-installation@1001.5.1
  - @pnpm/cli-utils@1000.0.10

## 1000.0.11

### Patch Changes

- Updated dependencies [91d46ee]
  - @pnpm/plugin-commands-installation@1001.5.0
  - @pnpm/cli-utils@1000.0.9

## 1000.0.10

### Patch Changes

- Updated dependencies [f6006f2]
  - @pnpm/plugin-commands-installation@1001.4.0
  - @pnpm/config@1002.3.0
  - @pnpm/cli-utils@1000.0.8

## 1000.0.9

### Patch Changes

- @pnpm/plugin-commands-installation@1001.3.2

## 1000.0.8

### Patch Changes

- Updated dependencies [1e229d7]
  - @pnpm/read-project-manifest@1000.0.5
  - @pnpm/cli-utils@1000.0.7
  - @pnpm/config@1002.2.1
  - @pnpm/link-bins@1000.0.6
  - @pnpm/plugin-commands-installation@1001.3.1
  - @pnpm/client@1000.0.6

## 1000.0.7

### Patch Changes

- Updated dependencies [f3ffaed]
- Updated dependencies [c96eb2b]
  - @pnpm/plugin-commands-installation@1001.3.0
  - @pnpm/config@1002.2.0
  - @pnpm/error@1000.0.2
  - @pnpm/cli-meta@1000.0.2
  - @pnpm/cli-utils@1000.0.6
  - @pnpm/pick-registry-for-package@1000.0.2
  - @pnpm/client@1000.0.5
  - @pnpm/link-bins@1000.0.5
  - @pnpm/read-project-manifest@1000.0.4

## 1000.0.6

### Patch Changes

- Updated dependencies [e050221]
- Updated dependencies [e050221]
  - @pnpm/read-project-manifest@1000.0.3
  - @pnpm/plugin-commands-installation@1001.2.1
  - @pnpm/cli-utils@1000.0.5
  - @pnpm/config@1002.1.2
  - @pnpm/link-bins@1000.0.4
  - @pnpm/client@1000.0.4

## 1000.0.5

### Patch Changes

- Updated dependencies [c7eefdd]
- Updated dependencies [9591a18]
- Updated dependencies [1f5169f]
  - @pnpm/plugin-commands-installation@1001.2.0
  - @pnpm/config@1002.1.1
  - @pnpm/cli-meta@1000.0.1
  - @pnpm/cli-utils@1000.0.4
  - @pnpm/pick-registry-for-package@1000.0.1
  - @pnpm/client@1000.0.3
  - @pnpm/link-bins@1000.0.3
  - @pnpm/read-project-manifest@1000.0.2

## 1000.0.4

### Patch Changes

- Updated dependencies [f90a94b]
- Updated dependencies [f891288]
- Updated dependencies [f891288]
  - @pnpm/config@1002.1.0
  - @pnpm/plugin-commands-installation@1001.1.0
  - @pnpm/cli-utils@1000.0.3

## 1000.0.3

### Patch Changes

- Updated dependencies [f685565]
- Updated dependencies [878ea8c]
  - @pnpm/plugin-commands-installation@1001.0.2
  - @pnpm/config@1002.0.0
  - @pnpm/cli-utils@1000.0.2
  - @pnpm/client@1000.0.2
  - @pnpm/link-bins@1000.0.2

## 1000.0.2

### Patch Changes

- @pnpm/plugin-commands-installation@1001.0.1

## 1000.0.1

### Patch Changes

- Updated dependencies [ac5b9d8]
- Updated dependencies [6483b64]
- Updated dependencies [31911f1]
- Updated dependencies [b8bda0a]
- Updated dependencies [d47c426]
- Updated dependencies [a76da0c]
  - @pnpm/plugin-commands-installation@1001.0.0
  - @pnpm/config@1001.0.0
  - @pnpm/cli-utils@1000.0.1
  - @pnpm/error@1000.0.1
  - @pnpm/client@1000.0.1
  - @pnpm/link-bins@1000.0.1
  - @pnpm/read-project-manifest@1000.0.1

## 1.1.0

### Minor Changes

- b530840: The `self-update` now accepts a version specifier to install a specific version of pnpm. E.g.: `pnpm self-update 9.5.0` or `pnpm self-update next-10`.

### Patch Changes

- Updated dependencies [477e0c1]
- Updated dependencies [dfcf034]
- Updated dependencies [592e2ef]
- Updated dependencies [19d5b51]
- Updated dependencies [19d5b51]
- Updated dependencies [1dbc56a]
- Updated dependencies [6b27c81]
- Updated dependencies [e9985b6]
  - @pnpm/plugin-commands-installation@18.0.0
  - @pnpm/config@22.0.0
  - @pnpm/error@6.0.3
  - @pnpm/cli-utils@4.0.8
  - @pnpm/link-bins@10.0.12
  - @pnpm/read-project-manifest@6.0.10
  - @pnpm/client@11.1.13

## 1.0.9

### Patch Changes

- Updated dependencies [6014522]
  - @pnpm/plugin-commands-installation@17.2.7
  - @pnpm/client@11.1.12
  - @pnpm/cli-utils@4.0.7
  - @pnpm/config@21.8.5
  - @pnpm/link-bins@10.0.11

## 1.0.8

### Patch Changes

- @pnpm/plugin-commands-installation@17.2.6
- @pnpm/client@11.1.11
- @pnpm/cli-utils@4.0.6
- @pnpm/config@21.8.4
- @pnpm/link-bins@10.0.11

## 1.0.7

### Patch Changes

- @pnpm/plugin-commands-installation@17.2.5

## 1.0.6

### Patch Changes

- Updated dependencies [83681da]
  - @pnpm/plugin-commands-installation@17.2.4
  - @pnpm/config@21.8.4
  - @pnpm/error@6.0.2
  - @pnpm/cli-utils@4.0.6
  - @pnpm/link-bins@10.0.11
  - @pnpm/read-project-manifest@6.0.9
  - @pnpm/client@11.1.10

## 1.0.5

### Patch Changes

- Updated dependencies [ad1fd64]
- Updated dependencies [eeb76cd]
  - @pnpm/plugin-commands-installation@17.2.3

## 1.0.4

### Patch Changes

- @pnpm/cli-meta@6.2.2
- @pnpm/cli-utils@4.0.5
- @pnpm/config@21.8.3
- @pnpm/pick-registry-for-package@6.0.7
- @pnpm/client@11.1.9
- @pnpm/link-bins@10.0.10
- @pnpm/plugin-commands-installation@17.2.2
- @pnpm/read-project-manifest@6.0.8

## 1.0.3

### Patch Changes

- @pnpm/plugin-commands-installation@17.2.1

## 1.0.2

### Patch Changes

- Updated dependencies [7ee59a1]
  - @pnpm/plugin-commands-installation@17.2.0
  - @pnpm/cli-meta@6.2.1
  - @pnpm/cli-utils@4.0.4
  - @pnpm/config@21.8.2
  - @pnpm/pick-registry-for-package@6.0.6
  - @pnpm/client@11.1.8
  - @pnpm/link-bins@10.0.9
  - @pnpm/read-project-manifest@6.0.7

## 1.0.1

### Patch Changes

- @pnpm/plugin-commands-installation@17.1.1

## 1.0.0

### Major Changes

- eb8bf2a: Added a new command for upgrading pnpm itself when it isn't managed by Corepack: `pnpm self-update`. This command will work, when pnpm was installed via the standalone script from the [pnpm installation page](https://pnpm.io/installation#using-a-standalone-script) [#8424](https://github.com/pnpm/pnpm/pull/8424).

  When executed in a project that has a `packageManager` field in its `package.json` file, pnpm will update its version in the `packageManager` field.

### Patch Changes

- Updated dependencies [eb8bf2a]
  - @pnpm/tools.path@1.0.0
  - @pnpm/plugin-commands-installation@17.1.0
  - @pnpm/cli-meta@6.2.0
  - @pnpm/cli-utils@4.0.3
