# @pnpm/plugin-commands-audit

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
