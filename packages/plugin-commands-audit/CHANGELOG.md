# @pnpm/plugin-commands-audit

## 5.1.12

### Patch Changes

- Updated dependencies [fe5688dc0]
- Updated dependencies [c7081cbb4]
- Updated dependencies [c7081cbb4]
  - @pnpm/config@13.0.0
  - @pnpm/cli-utils@0.6.23

## 5.1.11

### Patch Changes

- Updated dependencies [d62259d67]
  - @pnpm/config@12.6.0
  - @pnpm/cli-utils@0.6.22

## 5.1.10

### Patch Changes

- Updated dependencies [6681fdcbc]
  - @pnpm/config@12.5.0
  - @pnpm/cli-utils@0.6.21
  - @pnpm/audit@2.1.9

## 5.1.9

### Patch Changes

- @pnpm/audit@2.1.8
- @pnpm/cli-utils@0.6.20

## 5.1.8

### Patch Changes

- Updated dependencies [ede519190]
  - @pnpm/config@12.4.9
  - @pnpm/cli-utils@0.6.19

## 5.1.7

### Patch Changes

- @pnpm/config@12.4.8
- @pnpm/cli-utils@0.6.18

## 5.1.6

### Patch Changes

- Updated dependencies [655af55ba]
  - @pnpm/config@12.4.7
  - @pnpm/cli-utils@0.6.17

## 5.1.5

### Patch Changes

- Updated dependencies [3fb74c618]
  - @pnpm/config@12.4.6
  - @pnpm/cli-utils@0.6.16

## 5.1.4

### Patch Changes

- 92ed1272e: If a package has no fixes, do not add it to the overrides.
- Updated dependencies [051296a16]
  - @pnpm/config@12.4.5
  - @pnpm/cli-utils@0.6.15

## 5.1.3

### Patch Changes

- Updated dependencies [af8b5716e]
  - @pnpm/config@12.4.4
  - @pnpm/cli-utils@0.6.14

## 5.1.2

### Patch Changes

- @pnpm/audit@2.1.7
- @pnpm/cli-utils@0.6.13
- @pnpm/config@12.4.3
- @pnpm/lockfile-file@4.1.1
- @pnpm/read-project-manifest@2.0.5

## 5.1.1

### Patch Changes

- Updated dependencies [73c1f802e]
  - @pnpm/config@12.4.2
  - @pnpm/cli-utils@0.6.12

## 5.1.0

### Minor Changes

- a5f698290: New command added: `pnpm audit --fix`. This command adds overrides to `package.json` that force versions of packages that do not have the vulnerabilities.

### Patch Changes

- @pnpm/cli-utils@0.6.11

## 5.0.1

### Patch Changes

- Updated dependencies [2264bfdf4]
  - @pnpm/config@12.4.1
  - @pnpm/cli-utils@0.6.10

## 5.0.0

### Major Changes

- 691f64713: New required option added: cacheDir.

### Patch Changes

- Updated dependencies [25f6968d4]
- Updated dependencies [5aaf3e3fa]
  - @pnpm/config@12.4.0
  - @pnpm/cli-utils@0.6.9

## 4.2.2

### Patch Changes

- Updated dependencies [8e76690f4]
  - @pnpm/lockfile-file@4.1.0
  - @pnpm/audit@2.1.6
  - @pnpm/cli-utils@0.6.8
  - @pnpm/config@12.3.3

## 4.2.1

### Patch Changes

- @pnpm/audit@2.1.5

## 4.2.0

### Minor Changes

- 448710f88: New CLI option added: `--ignore-registry-errors`. When used, audit exits with 0 exit code, when the registry responds with a non-200 status code.

### Patch Changes

- Updated dependencies [2dc5a7a4c]
  - @pnpm/lockfile-file@4.0.4
  - @pnpm/audit@2.1.4

## 4.1.6

### Patch Changes

- @pnpm/audit@2.1.4
- @pnpm/cli-utils@0.6.7
- @pnpm/config@12.3.2
- @pnpm/lockfile-file@4.0.3

## 4.1.5

### Patch Changes

- a1a03d145: Import only the required functions from ramda.
- Updated dependencies [a1a03d145]
  - @pnpm/config@12.3.1
  - @pnpm/lockfile-file@4.0.2
  - @pnpm/cli-utils@0.6.6
  - @pnpm/audit@2.1.3

## 4.1.4

### Patch Changes

- Updated dependencies [84ec82e05]
- Updated dependencies [c2a71e4fd]
- Updated dependencies [84ec82e05]
  - @pnpm/config@12.3.0
  - @pnpm/cli-utils@0.6.5

## 4.1.3

### Patch Changes

- @pnpm/cli-utils@0.6.4
- @pnpm/audit@2.1.2

## 4.1.2

### Patch Changes

- @pnpm/cli-utils@0.6.3
- @pnpm/config@12.2.0

## 4.1.1

### Patch Changes

- Updated dependencies [40b75fbb9]
  - @pnpm/audit@2.1.1
  - @pnpm/config@12.2.0

## 4.1.0

### Minor Changes

- 05baaa6e7: Add new config setting: `fetch-timeout`.

### Patch Changes

- Updated dependencies [05baaa6e7]
- Updated dependencies [dfdf669e6]
- Updated dependencies [05baaa6e7]
  - @pnpm/config@12.2.0
  - @pnpm/audit@2.1.0
  - @pnpm/cli-utils@0.6.2
  - @pnpm/lockfile-file@4.0.1

## 4.0.2

### Patch Changes

- Updated dependencies [ba5231ccf]
  - @pnpm/config@12.1.0
  - @pnpm/cli-utils@0.6.1

## 4.0.1

### Patch Changes

- @pnpm/audit@2.0.1

## 4.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Patch Changes

- Updated dependencies [6871d74b2]
- Updated dependencies [97b986fbc]
- Updated dependencies [78470a32d]
- Updated dependencies [155e70597]
- Updated dependencies [9c2a878c3]
- Updated dependencies [aed712455]
- Updated dependencies [8b66f26dc]
- Updated dependencies [f2bb5cbeb]
- Updated dependencies [f7750baed]
- Updated dependencies [aed712455]
- Updated dependencies [9c2a878c3]
  - @pnpm/constants@5.0.0
  - @pnpm/audit@2.0.0
  - @pnpm/cli-utils@0.6.0
  - @pnpm/config@12.0.0
  - @pnpm/error@2.0.0
  - @pnpm/lockfile-file@4.0.0

## 3.0.6

### Patch Changes

- Updated dependencies [4f1ce907a]
  - @pnpm/config@11.14.2
  - @pnpm/cli-utils@0.5.4

## 3.0.5

### Patch Changes

- Updated dependencies [4b3852c39]
  - @pnpm/config@11.14.1
  - @pnpm/cli-utils@0.5.3

## 3.0.4

### Patch Changes

- @pnpm/audit@1.1.24

## 3.0.3

### Patch Changes

- @pnpm/config@11.14.0
- @pnpm/cli-utils@0.5.2

## 3.0.2

### Patch Changes

- Updated dependencies [3be2b1773]
  - @pnpm/cli-utils@0.5.1

## 3.0.1

### Patch Changes

- Updated dependencies [51e1456dd]
  - @pnpm/lockfile-file@3.2.1
  - @pnpm/audit@1.1.23

## 3.0.0

### Major Changes

- 5175460a0: Filter dependency types via the `dev`/`production`/`optional` options instead of the `included` option.

## 2.0.43

### Patch Changes

- 0c11e1a07: Audit output should always have a new line at the end.
- Updated dependencies [cb040ae18]
  - @pnpm/cli-utils@0.5.0
  - @pnpm/config@11.14.0

## 2.0.42

### Patch Changes

- Updated dependencies [c4cc62506]
  - @pnpm/config@11.13.0
  - @pnpm/cli-utils@0.4.51

## 2.0.41

### Patch Changes

- Updated dependencies [bff84dbca]
  - @pnpm/config@11.12.1
  - @pnpm/cli-utils@0.4.50

## 2.0.40

### Patch Changes

- @pnpm/cli-utils@0.4.49

## 2.0.39

### Patch Changes

- @pnpm/cli-utils@0.4.48

## 2.0.38

### Patch Changes

- Updated dependencies [9ad8c27bf]
- Updated dependencies [548f28df9]
- Updated dependencies [548f28df9]
  - @pnpm/lockfile-file@3.2.0
  - @pnpm/cli-utils@0.4.47
  - @pnpm/config@11.12.0
  - @pnpm/audit@1.1.23

## 2.0.37

### Patch Changes

- @pnpm/config@11.11.1
- @pnpm/cli-utils@0.4.46

## 2.0.36

### Patch Changes

- Updated dependencies [af897c324]
  - @pnpm/lockfile-file@3.1.4
  - @pnpm/audit@1.1.22

## 2.0.35

### Patch Changes

- Updated dependencies [1e4a3a17a]
- Updated dependencies [f40bc5927]
  - @pnpm/lockfile-file@3.1.3
  - @pnpm/config@11.11.0
  - @pnpm/audit@1.1.22
  - @pnpm/cli-utils@0.4.45

## 2.0.34

### Patch Changes

- Updated dependencies [425c7547d]
  - @pnpm/config@11.10.2
  - @pnpm/cli-utils@0.4.44
  - @pnpm/audit@1.1.22

## 2.0.33

### Patch Changes

- Updated dependencies [ea09da716]
  - @pnpm/config@11.10.1
  - @pnpm/cli-utils@0.4.43

## 2.0.32

### Patch Changes

- Updated dependencies [a8656b42f]
  - @pnpm/config@11.10.0
  - @pnpm/cli-utils@0.4.42

## 2.0.31

### Patch Changes

- Updated dependencies [041537bc3]
  - @pnpm/config@11.9.1
  - @pnpm/cli-utils@0.4.41

## 2.0.30

### Patch Changes

- Updated dependencies [fba715512]
  - @pnpm/lockfile-file@3.1.2
  - @pnpm/audit@1.1.21

## 2.0.29

### Patch Changes

- @pnpm/audit@1.1.20

## 2.0.28

### Patch Changes

- Updated dependencies [8698a7060]
  - @pnpm/config@11.9.0
  - @pnpm/cli-utils@0.4.40
  - @pnpm/audit@1.1.19

## 2.0.27

### Patch Changes

- Updated dependencies [fcc1c7100]
  - @pnpm/config@11.8.0
  - @pnpm/cli-utils@0.4.39

## 2.0.26

### Patch Changes

- Updated dependencies [0c5f1bcc9]
  - @pnpm/error@1.4.0
  - @pnpm/audit@1.1.18
  - @pnpm/cli-utils@0.4.38
  - @pnpm/config@11.7.2
  - @pnpm/lockfile-file@3.1.1

## 2.0.25

### Patch Changes

- Updated dependencies [3776b5a52]
  - @pnpm/lockfile-file@3.1.0
  - @pnpm/audit@1.1.17

## 2.0.24

### Patch Changes

- Updated dependencies [dbcc6c96f]
- Updated dependencies [09492b7b4]
  - @pnpm/lockfile-file@3.0.18
  - @pnpm/audit@1.1.17
  - @pnpm/cli-utils@0.4.37

## 2.0.23

### Patch Changes

- e70232907: Use @arcanis/slice-ansi instead of slice-ansi.
- Updated dependencies [aa6bc4f95]
  - @pnpm/lockfile-file@3.0.17
  - @pnpm/audit@1.1.17
  - @pnpm/cli-utils@0.4.36

## 2.0.22

### Patch Changes

- @pnpm/audit@1.1.16
- @pnpm/lockfile-file@3.0.16
- @pnpm/cli-utils@0.4.35
- @pnpm/config@11.7.1

## 2.0.21

### Patch Changes

- Updated dependencies [50b360ec1]
  - @pnpm/config@11.7.0
  - @pnpm/cli-utils@0.4.34

## 2.0.20

### Patch Changes

- Updated dependencies [fcdad632f]
  - @pnpm/constants@4.1.0
  - @pnpm/audit@1.1.15
  - @pnpm/lockfile-file@3.0.15
  - @pnpm/cli-utils@0.4.33
  - @pnpm/config@11.6.1

## 2.0.19

### Patch Changes

- Updated dependencies [f591fdeeb]
  - @pnpm/config@11.6.0
  - @pnpm/cli-utils@0.4.32

## 2.0.18

### Patch Changes

- @pnpm/cli-utils@0.4.31

## 2.0.17

### Patch Changes

- Updated dependencies [74914c178]
  - @pnpm/config@11.5.0
  - @pnpm/cli-utils@0.4.30

## 2.0.16

### Patch Changes

- Updated dependencies [23cf3c88b]
  - @pnpm/config@11.4.0
  - @pnpm/cli-utils@0.4.29

## 2.0.15

### Patch Changes

- Updated dependencies [767212f4e]
- Updated dependencies [092f8dd83]
  - @pnpm/config@11.3.0
  - @pnpm/cli-utils@0.4.28

## 2.0.14

### Patch Changes

- @pnpm/audit@1.1.14
- @pnpm/cli-utils@0.4.27

## 2.0.13

### Patch Changes

- @pnpm/audit@1.1.13

## 2.0.12

### Patch Changes

- @pnpm/cli-utils@0.4.26

## 2.0.11

### Patch Changes

- Updated dependencies [75a36deba]
- Updated dependencies [9f1a29ff9]
  - @pnpm/error@1.3.1
  - @pnpm/config@11.2.7
  - @pnpm/audit@1.1.12
  - @pnpm/cli-utils@0.4.25
  - @pnpm/lockfile-file@3.0.14

## 2.0.10

### Patch Changes

- 6138b56d0: Update table to v6.

## 2.0.9

### Patch Changes

- Updated dependencies [ac0d3e122]
  - @pnpm/config@11.2.6
  - @pnpm/cli-utils@0.4.24

## 2.0.8

### Patch Changes

- Updated dependencies [9550b0505]
- Updated dependencies [972864e0d]
  - @pnpm/lockfile-file@3.0.13
  - @pnpm/config@11.2.5
  - @pnpm/audit@1.1.11
  - @pnpm/cli-utils@0.4.23

## 2.0.7

### Patch Changes

- Updated dependencies [6d480dd7a]
  - @pnpm/error@1.3.0
  - @pnpm/audit@1.1.11
  - @pnpm/cli-utils@0.4.22
  - @pnpm/config@11.2.4
  - @pnpm/lockfile-file@3.0.12

## 2.0.6

### Patch Changes

- Updated dependencies [13c18e397]
  - @pnpm/config@11.2.3
  - @pnpm/cli-utils@0.4.21

## 2.0.5

### Patch Changes

- Updated dependencies [3f6d35997]
  - @pnpm/config@11.2.2
  - @pnpm/cli-utils@0.4.20

## 2.0.4

### Patch Changes

- @pnpm/cli-utils@0.4.19

## 2.0.3

### Patch Changes

- @pnpm/cli-utils@0.4.18

## 2.0.2

### Patch Changes

- Updated dependencies [a2ef8084f]
  - @pnpm/config@11.2.1
  - @pnpm/audit@1.1.10
  - @pnpm/cli-utils@0.4.17

## 2.0.1

### Patch Changes

- 8bb015059: `pnpm audit --audit-level high` should not error if the found vulnerabilities are low and/or moderate.

## 2.0.0

### Major Changes

- a64b7250c: Return `Promise&lt;{ output: string, exitCode: number }>` instead of `Promise&lt;string>`.

  `exitCode` is `1` when there are any packages with vulnerabilities in the dependencies.

## 1.0.21

### Patch Changes

- Updated dependencies [ad69677a7]
  - @pnpm/cli-utils@0.4.16
  - @pnpm/config@11.2.0

## 1.0.20

### Patch Changes

- 4e5e22aab: Allow to set a custom registry through the `--registry` option, when running `pnpm audit` (#2689).

## 1.0.19

### Patch Changes

- @pnpm/audit@1.1.9
- @pnpm/cli-utils@0.4.15

## 1.0.18

### Patch Changes

- Updated dependencies [65b4d07ca]
- Updated dependencies [ab3b8f51d]
  - @pnpm/config@11.1.0
  - @pnpm/cli-utils@0.4.14
  - @pnpm/audit@1.1.8

## 1.0.17

### Patch Changes

- @pnpm/config@11.0.1
- @pnpm/cli-utils@0.4.13

## 1.0.16

### Patch Changes

- Updated dependencies [71aeb9a38]
- Updated dependencies [915828b46]
  - @pnpm/config@11.0.0
  - @pnpm/cli-utils@0.4.12
  - @pnpm/audit@1.1.7

## 1.0.15

### Patch Changes

- @pnpm/config@10.0.1
- @pnpm/cli-utils@0.4.11

## 1.0.14

### Patch Changes

- Updated dependencies [db17f6f7b]
- Updated dependencies [1146b76d2]
  - @pnpm/config@10.0.0
  - @pnpm/cli-utils@0.4.10
  - @pnpm/audit@1.1.6
  - @pnpm/lockfile-file@3.0.11

## 1.0.13

### Patch Changes

- Updated dependencies [71a8c8ce3]
  - @pnpm/config@9.2.0
  - @pnpm/audit@1.1.5
  - @pnpm/cli-utils@0.4.9
  - @pnpm/lockfile-file@3.0.10

## 1.0.12

### Patch Changes

- e934b1a48: Update chalk to v4.1.0.
- Updated dependencies [e934b1a48]
  - @pnpm/cli-utils@0.4.8
  - @pnpm/audit@1.1.4

## 1.0.11

### Patch Changes

- @pnpm/audit@1.1.3
- @pnpm/cli-utils@0.4.7

## 1.0.10

### Patch Changes

- Updated dependencies [ffddf34a8]
  - @pnpm/config@9.1.0
  - @pnpm/cli-utils@0.4.6

## 1.0.9

### Patch Changes

- Updated dependencies [b5f66c0f2]
- Updated dependencies [242cf8737]
- Updated dependencies [ca9f50844]
- Updated dependencies [e11019b89]
- Updated dependencies [802d145fc]
- Updated dependencies [45fdcfde2]
- Updated dependencies [4f5801b1c]
  - @pnpm/constants@4.0.0
  - @pnpm/config@9.0.0
  - @pnpm/audit@1.1.2
  - @pnpm/cli-utils@0.4.5
  - @pnpm/error@1.2.1
  - @pnpm/lockfile-file@3.0.9

## 1.0.9-alpha.2

### Patch Changes

- Updated dependencies [242cf8737]
- Updated dependencies [ca9f50844]
- Updated dependencies [45fdcfde2]
  - @pnpm/config@9.0.0-alpha.2
  - @pnpm/constants@4.0.0-alpha.1
  - @pnpm/cli-utils@0.4.5-alpha.2
  - @pnpm/lockfile-file@3.0.9-alpha.2
  - @pnpm/audit@1.1.2-alpha.2

## 1.0.9-alpha.1

### Patch Changes

- @pnpm/audit@1.1.2-alpha.1
- @pnpm/cli-utils@0.4.5-alpha.1
- @pnpm/config@8.3.1-alpha.1
- @pnpm/lockfile-file@3.0.9-alpha.1

## 1.0.9-alpha.0

### Patch Changes

- Updated dependencies [b5f66c0f2]
  - @pnpm/constants@4.0.0-alpha.0
  - @pnpm/config@8.3.1-alpha.0
  - @pnpm/lockfile-file@3.0.9-alpha.0
  - @pnpm/cli-utils@0.4.5-alpha.0
  - @pnpm/audit@1.1.1-alpha.0

## 1.0.8

### Patch Changes

- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
- Updated dependencies [907c63a48]
  - @pnpm/lockfile-file@3.0.8
  - @pnpm/audit@1.1.0
  - @pnpm/cli-utils@0.4.4
