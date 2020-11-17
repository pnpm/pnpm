# @pnpm/plugin-commands-script-runners

## 2.5.6

### Patch Changes

- Updated dependencies [8698a7060]
  - @pnpm/config@11.9.0
  - @pnpm/cli-utils@0.4.40

## 2.5.5

### Patch Changes

- Updated dependencies [fcc1c7100]
  - @pnpm/config@11.8.0
  - @pnpm/cli-utils@0.4.39

## 2.5.4

### Patch Changes

- Updated dependencies [0c5f1bcc9]
  - @pnpm/error@1.4.0
  - @pnpm/cli-utils@0.4.38
  - @pnpm/config@11.7.2
  - @pnpm/lifecycle@9.6.2

## 2.5.3

### Patch Changes

- @pnpm/cli-utils@0.4.37

## 2.5.2

### Patch Changes

- @pnpm/cli-utils@0.4.36

## 2.5.1

### Patch Changes

- Updated dependencies [b5d694e7f]
  - @pnpm/types@6.3.1
  - @pnpm/cli-utils@0.4.35
  - @pnpm/config@11.7.1
  - @pnpm/lifecycle@9.6.1
  - @pnpm/sort-packages@1.0.15

## 2.5.0

### Minor Changes

- 50b360ec1: A new option added for specifying the shell to use, when running scripts: scriptShell.

### Patch Changes

- Updated dependencies [50b360ec1]
  - @pnpm/config@11.7.0
  - @pnpm/lifecycle@9.6.0
  - @pnpm/cli-utils@0.4.34

## 2.4.1

### Patch Changes

- Updated dependencies [d54043ee4]
  - @pnpm/types@6.3.0
  - @pnpm/cli-utils@0.4.33
  - @pnpm/config@11.6.1
  - @pnpm/lifecycle@9.5.1
  - @pnpm/sort-packages@1.0.14

## 2.4.0

### Minor Changes

- f591fdeeb: Scripts support Plug'n'Play.

### Patch Changes

- Updated dependencies [f591fdeeb]
- Updated dependencies [f591fdeeb]
- Updated dependencies [f591fdeeb]
  - @pnpm/config@11.6.0
  - @pnpm/lifecycle@9.5.0
  - @pnpm/cli-utils@0.4.32

## 2.3.3

### Patch Changes

- @pnpm/cli-utils@0.4.31

## 2.3.2

### Patch Changes

- Updated dependencies [74914c178]
  - @pnpm/config@11.5.0
  - @pnpm/cli-utils@0.4.30

## 2.3.1

### Patch Changes

- Updated dependencies [203e65ac8]
  - @pnpm/lifecycle@9.4.0

## 2.3.0

### Minor Changes

- 23cf3c88b: New option added: `shellEmulator`.

### Patch Changes

- Updated dependencies [23cf3c88b]
  - @pnpm/config@11.4.0
  - @pnpm/lifecycle@9.3.0
  - @pnpm/cli-utils@0.4.29

## 2.2.0

### Minor Changes

- 092f8dd83: When a script is not found but is present in the workspace root, suggest to use `pnpm -w run`.
- 092f8dd83: `pnpm run` prints all scripts from the root of the workspace. They may be executed using `pnpm -w run`.

### Patch Changes

- Updated dependencies [767212f4e]
- Updated dependencies [092f8dd83]
- Updated dependencies [092f8dd83]
  - @pnpm/config@11.3.0
  - @pnpm/common-cli-options-help@0.2.0
  - @pnpm/cli-utils@0.4.28

## 2.1.0

### Minor Changes

- d11442a57: If a script is not found in the current project but is present in the root project of the workspace, notify the user about it in the hint of the error.

### Patch Changes

- @pnpm/lifecycle@9.2.5
- @pnpm/cli-utils@0.4.27

## 2.0.1

### Patch Changes

- @pnpm/cli-utils@0.4.26

## 2.0.0

### Major Changes

- de61940a5: The start and stop script commands are removed.
  There is no reason to define separate handlers for shorthand commands
  as any unknown command is automatically converted to a script.

### Patch Changes

- de61940a5: `pnpm test|start|stop` support the same options as `pnpm run test|start|stop`.
- Updated dependencies [75a36deba]
- Updated dependencies [9f1a29ff9]
  - @pnpm/error@1.3.1
  - @pnpm/config@11.2.7
  - @pnpm/cli-utils@0.4.25
  - @pnpm/lifecycle@9.2.4

## 1.2.19

### Patch Changes

- Updated dependencies [ac0d3e122]
  - @pnpm/config@11.2.6
  - @pnpm/cli-utils@0.4.24

## 1.2.18

### Patch Changes

- Updated dependencies [972864e0d]
  - @pnpm/config@11.2.5
  - @pnpm/lifecycle@9.2.3
  - @pnpm/cli-utils@0.4.23

## 1.2.17

### Patch Changes

- Updated dependencies [6d480dd7a]
  - @pnpm/error@1.3.0
  - @pnpm/cli-utils@0.4.22
  - @pnpm/config@11.2.4

## 1.2.16

### Patch Changes

- Updated dependencies [13c18e397]
  - @pnpm/config@11.2.3
  - @pnpm/cli-utils@0.4.21

## 1.2.15

### Patch Changes

- Updated dependencies [3f6d35997]
  - @pnpm/config@11.2.2
  - @pnpm/cli-utils@0.4.20

## 1.2.14

### Patch Changes

- @pnpm/cli-utils@0.4.19

## 1.2.13

### Patch Changes

- @pnpm/cli-utils@0.4.18

## 1.2.12

### Patch Changes

- a2ef8084f: Use the same versions of dependencies across the pnpm monorepo.
- Updated dependencies [a2ef8084f]
  - @pnpm/config@11.2.1
  - @pnpm/lifecycle@9.2.2
  - @pnpm/cli-utils@0.4.17

## 1.2.11

### Patch Changes

- Updated dependencies [ad69677a7]
  - @pnpm/cli-utils@0.4.16
  - @pnpm/config@11.2.0

## 1.2.10

### Patch Changes

- @pnpm/lifecycle@9.2.1
- @pnpm/cli-utils@0.4.15

## 1.2.9

### Patch Changes

- Updated dependencies [65b4d07ca]
- Updated dependencies [ab3b8f51d]
  - @pnpm/config@11.1.0
  - @pnpm/cli-utils@0.4.14

## 1.2.8

### Patch Changes

- 76aaead32: `run --silent <cmd>` should only print output of the command and nothing from pnpm.
- Updated dependencies [76aaead32]
  - @pnpm/lifecycle@9.2.0

## 1.2.7

### Patch Changes

- @pnpm/config@11.0.1
- @pnpm/cli-utils@0.4.13

## 1.2.6

### Patch Changes

- Updated dependencies [71aeb9a38]
- Updated dependencies [915828b46]
  - @pnpm/config@11.0.0
  - @pnpm/cli-utils@0.4.12

## 1.2.5

### Patch Changes

- @pnpm/config@10.0.1
- @pnpm/cli-utils@0.4.11

## 1.2.4

### Patch Changes

- 220896511: Remove common-tags from dependencies.
- Updated dependencies [db17f6f7b]
- Updated dependencies [1146b76d2]
- Updated dependencies [db17f6f7b]
  - @pnpm/config@10.0.0
  - @pnpm/types@6.2.0
  - @pnpm/cli-utils@0.4.10
  - @pnpm/lifecycle@9.1.3
  - @pnpm/sort-packages@1.0.13

## 1.2.3

### Patch Changes

- Updated dependencies [71a8c8ce3]
- Updated dependencies [71a8c8ce3]
  - @pnpm/types@6.1.0
  - @pnpm/config@9.2.0
  - @pnpm/cli-utils@0.4.9
  - @pnpm/lifecycle@9.1.2
  - @pnpm/sort-packages@1.0.12

## 1.2.2

### Patch Changes

- Updated dependencies [e934b1a48]
  - @pnpm/cli-utils@0.4.8

## 1.2.1

### Patch Changes

- d3ddd023c: Update p-limit to v3.
- Updated dependencies [d3ddd023c]
- Updated dependencies [68d8dc68f]
  - @pnpm/lifecycle@9.1.1
  - @pnpm/cli-utils@0.4.7

## 1.2.0

### Minor Changes

- ffddf34a8: Add new global option called `--stream`.
  When used, the output from child processes is streamed to the console immediately, prefixed with the originating package directory. This allows output from different packages to be interleaved.
- 0e8daafe4: The `run` and `exec` commands may use the `--parallel` option.

  `--parallel` completely disregards concurrency and topological sorting,
  running a given script immediately in all matching packages
  with prefixed streaming output. This is the preferred flag
  for long-running processes such as watch run over many packages.

  For example: `pnpm run --parallel watch`

### Patch Changes

- 8094b2a62: A recursive run should not rerun the same package script which started the lifecycle event.

  For instance, let's say one of the workspace projects has the following script:

  ```json
  "scripts": {
    "build": "pnpm run -r build"
  }
  ```

  Running `pnpm run build` in this project should not start an infinite recursion.
  `pnpm run -r build` in this case should run `build` in all the workspace projects except the one that started the build.

  Related issue: #2528

- Updated dependencies [ffddf34a8]
- Updated dependencies [ffddf34a8]
- Updated dependencies [8094b2a62]
  - @pnpm/common-cli-options-help@0.2.0
  - @pnpm/config@9.1.0
  - @pnpm/lifecycle@9.1.0
  - @pnpm/cli-utils@0.4.6
  - @pnpm/sort-packages@1.0.11

## 1.1.0

### Minor Changes

- 7300eba86: Support if-present flag for recursive run

### Patch Changes

- Updated dependencies [242cf8737]
- Updated dependencies [da091c711]
- Updated dependencies [f35a3ec1c]
- Updated dependencies [e11019b89]
- Updated dependencies [802d145fc]
- Updated dependencies [45fdcfde2]
- Updated dependencies [e3990787a]
  - @pnpm/config@9.0.0
  - @pnpm/types@6.0.0
  - @pnpm/lifecycle@9.0.0
  - @pnpm/cli-utils@0.4.5
  - @pnpm/command@1.0.1
  - @pnpm/common-cli-options-help@0.1.6
  - @pnpm/error@1.2.1
  - @pnpm/sort-packages@1.0.10

## 1.1.0-alpha.3

### Patch Changes

- Updated dependencies [242cf8737]
- Updated dependencies [45fdcfde2]
  - @pnpm/config@9.0.0-alpha.2
  - @pnpm/cli-utils@0.4.5-alpha.2
  - @pnpm/sort-packages@1.0.10-alpha.2

## 1.1.0-alpha.2

### Patch Changes

- Updated dependencies [da091c71]
- Updated dependencies [e3990787]
  - @pnpm/types@6.0.0-alpha.0
  - @pnpm/lifecycle@9.0.0-alpha.1
  - @pnpm/cli-utils@0.4.5-alpha.1
  - @pnpm/config@8.3.1-alpha.1
  - @pnpm/sort-packages@1.0.10-alpha.1

## 1.1.0-alpha.1

### Patch Changes

- @pnpm/config@8.3.1-alpha.0
- @pnpm/cli-utils@0.4.5-alpha.0
- @pnpm/sort-packages@1.0.10-alpha.0

## 1.1.0-alpha.0

### Minor Changes

- 7300eba86: Support if-present flag for recursive run

### Patch Changes

- Updated dependencies [f35a3ec1c]
  - @pnpm/lifecycle@8.2.0-alpha.0

## 1.1.0

### Minor Changes

- c80d4ba3c: Support if-present flag for recursive run

### Patch Changes

- Updated dependencies [2ec4c4eb9]
  - @pnpm/lifecycle@8.2.0

## 1.0.8

### Patch Changes

- 907c63a48: Dependencies updated.
  - @pnpm/cli-utils@0.4.4
