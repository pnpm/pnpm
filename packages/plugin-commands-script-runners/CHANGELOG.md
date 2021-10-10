# @pnpm/plugin-commands-script-runners

## 4.3.0

### Minor Changes

- c83488d01: New command added: create. `pnpm create` is similar to `yarn create`.
- 1efaaf706: `pnpm dlx` supports the `--silent` option.

### Patch Changes

- 091ff5f12: Add link to the docs into the help output of dlx and exec.
- Updated dependencies [4027a3c69]
- Updated dependencies [1efaaf706]
  - @pnpm/config@13.1.0
  - @pnpm/common-cli-options-help@0.7.1
  - @pnpm/cli-utils@0.6.24

## 4.2.7

### Patch Changes

- Updated dependencies [4a4d42d8f]
  - @pnpm/lifecycle@11.0.5

## 4.2.6

### Patch Changes

- Updated dependencies [fe5688dc0]
- Updated dependencies [c7081cbb4]
- Updated dependencies [c7081cbb4]
  - @pnpm/common-cli-options-help@0.7.0
  - @pnpm/config@13.0.0
  - @pnpm/cli-utils@0.6.23

## 4.2.5

### Patch Changes

- Updated dependencies [d62259d67]
  - @pnpm/config@12.6.0
  - @pnpm/cli-utils@0.6.22

## 4.2.4

### Patch Changes

- 04b7f6086: Use safe-execa instead of execa to prevent binary planting attacks on Windows.

## 4.2.3

### Patch Changes

- Updated dependencies [6681fdcbc]
  - @pnpm/config@12.5.0
  - @pnpm/cli-utils@0.6.21

## 4.2.2

### Patch Changes

- @pnpm/cli-utils@0.6.20

## 4.2.1

### Patch Changes

- Updated dependencies [ede519190]
  - @pnpm/config@12.4.9
  - @pnpm/cli-utils@0.6.19

## 4.2.0

### Minor Changes

- 7f097f26f: Support for multiple `--package` parameters added for `pnpm dlx` command

### Patch Changes

- @pnpm/config@12.4.8
- @pnpm/cli-utils@0.6.18

## 4.1.2

### Patch Changes

- Updated dependencies [655af55ba]
  - @pnpm/config@12.4.7
  - @pnpm/cli-utils@0.6.17

## 4.1.1

### Patch Changes

- b17096a36: `pnpm dlx` should not fail when pnpm has no write access to the CWD.

## 4.1.0

### Minor Changes

- 376c30485: New command added for running packages in a tempory environment: `pnpm dlx <command> ...`

### Patch Changes

- bd442ecb5: fix: add "run" to NO_SCRIPT error example

## 4.0.8

### Patch Changes

- Updated dependencies [3fb74c618]
  - @pnpm/config@12.4.6
  - @pnpm/cli-utils@0.6.16

## 4.0.7

### Patch Changes

- Updated dependencies [051296a16]
  - @pnpm/config@12.4.5
  - @pnpm/cli-utils@0.6.15

## 4.0.6

### Patch Changes

- Updated dependencies [af8b5716e]
  - @pnpm/config@12.4.4
  - @pnpm/cli-utils@0.6.14

## 4.0.5

### Patch Changes

- Updated dependencies [b734b45ea]
  - @pnpm/types@7.4.0
  - @pnpm/cli-utils@0.6.13
  - @pnpm/config@12.4.3
  - @pnpm/lifecycle@11.0.4
  - @pnpm/read-project-manifest@2.0.5
  - @pnpm/sort-packages@2.1.1

## 4.0.4

### Patch Changes

- Updated dependencies [7af16a011]
- Updated dependencies [73c1f802e]
  - @pnpm/lifecycle@11.0.3
  - @pnpm/config@12.4.2
  - @pnpm/cli-utils@0.6.12

## 4.0.3

### Patch Changes

- @pnpm/cli-utils@0.6.11

## 4.0.2

### Patch Changes

- 9476d5ac5: `pnpm exec` should work outside of Node.js projects.

## 4.0.1

### Patch Changes

- Updated dependencies [2264bfdf4]
  - @pnpm/config@12.4.1
  - @pnpm/cli-utils@0.6.10

## 4.0.0

### Major Changes

- 691f64713: New required option added: cacheDir.

### Patch Changes

- Updated dependencies [25f6968d4]
- Updated dependencies [5aaf3e3fa]
  - @pnpm/config@12.4.0
  - @pnpm/cli-utils@0.6.9

## 3.3.2

### Patch Changes

- Updated dependencies [1442f8786]
- Updated dependencies [8e76690f4]
  - @pnpm/sort-packages@2.1.0
  - @pnpm/types@7.3.0
  - @pnpm/cli-utils@0.6.8
  - @pnpm/config@12.3.3
  - @pnpm/lifecycle@11.0.2
  - @pnpm/read-project-manifest@2.0.4

## 3.3.1

### Patch Changes

- 4add11a96: `pnpm exec` should be executed in the context of the current working directory.

## 3.3.0

### Minor Changes

- 06f127503: `--` is ignored, when it is passed in as the first parameter to the exec command. This is for backward compatibility.

### Patch Changes

- Updated dependencies [724c5abd8]
  - @pnpm/types@7.2.0
  - @pnpm/cli-utils@0.6.7
  - @pnpm/config@12.3.2
  - @pnpm/lifecycle@11.0.1
  - @pnpm/read-project-manifest@2.0.3
  - @pnpm/sort-packages@2.0.2

## 3.2.2

### Patch Changes

- a1a03d145: Import only the required functions from ramda.
- Updated dependencies [a1a03d145]
  - @pnpm/config@12.3.1
  - @pnpm/cli-utils@0.6.6

## 3.2.1

### Patch Changes

- a77a2005e: `pnpm exec` should exit with the exit code of the child process and should not print an error.

## 3.2.0

### Minor Changes

- 209c14235: `pnpm run` is passed through to `pnpm exec` when it detects a command that is not in the scripts.

### Patch Changes

- c1f137412: `pnpm exec` should add `node_modules/.bin` to the PATH.
- c1f137412: `pnpm exec` should add the Node.js location to the PATH.

## 3.1.6

### Patch Changes

- Updated dependencies [84ec82e05]
- Updated dependencies [c2a71e4fd]
- Updated dependencies [84ec82e05]
  - @pnpm/config@12.3.0
  - @pnpm/common-cli-options-help@0.6.0
  - @pnpm/cli-utils@0.6.5

## 3.1.5

### Patch Changes

- ff9714d78: Don't list the commands twice when `pnpm run` is executed in the root of a workspace.

## 3.1.4

### Patch Changes

- @pnpm/cli-utils@0.6.4

## 3.1.3

### Patch Changes

- @pnpm/cli-utils@0.6.3
- @pnpm/config@12.2.0

## 3.1.2

### Patch Changes

- Updated dependencies [e6a2654a2]
  - @pnpm/lifecycle@11.0.0
  - @pnpm/config@12.2.0

## 3.1.1

### Patch Changes

- Updated dependencies [05baaa6e7]
- Updated dependencies [dfdf669e6]
- Updated dependencies [97c64bae4]
  - @pnpm/config@12.2.0
  - @pnpm/common-cli-options-help@0.5.0
  - @pnpm/types@7.1.0
  - @pnpm/cli-utils@0.6.2
  - @pnpm/lifecycle@10.0.1
  - @pnpm/sort-packages@2.0.1

## 3.1.0

### Minor Changes

- ba5231ccf: New option added for: `enable-pre-post-scripts`. When it is set to `true`, lifecycle scripts with pre/post prefixes are automatically executed by pnpm.

### Patch Changes

- Updated dependencies [ba5231ccf]
  - @pnpm/config@12.1.0
  - @pnpm/cli-utils@0.6.1

## 3.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.
- 34338d2d0: Arbitrary pre/post hooks for user-defined scripts (such as `prestart`) are not executed automatically.
- 048c94871: `.pnp.js` renamed to `.pnp.cjs` in order to force CommonJS.

### Patch Changes

- Updated dependencies [97b986fbc]
- Updated dependencies [78470a32d]
- Updated dependencies [aed712455]
- Updated dependencies [aed712455]
  - @pnpm/cli-utils@0.6.0
  - @pnpm/command@2.0.0
  - @pnpm/common-cli-options-help@0.4.0
  - @pnpm/config@12.0.0
  - @pnpm/error@2.0.0
  - @pnpm/lifecycle@10.0.0
  - @pnpm/sort-packages@2.0.0
  - @pnpm/types@7.0.0

## 2.6.5

### Patch Changes

- Updated dependencies [4f1ce907a]
  - @pnpm/config@11.14.2
  - @pnpm/cli-utils@0.5.4

## 2.6.4

### Patch Changes

- Updated dependencies [d853fb14a]
- Updated dependencies [4b3852c39]
  - @pnpm/lifecycle@9.6.5
  - @pnpm/config@11.14.1
  - @pnpm/cli-utils@0.5.3

## 2.6.3

### Patch Changes

- @pnpm/config@11.14.0
- @pnpm/cli-utils@0.5.2

## 2.6.2

### Patch Changes

- Updated dependencies [3be2b1773]
  - @pnpm/cli-utils@0.5.1

## 2.6.1

### Patch Changes

- Updated dependencies [a5e9d903c]
- Updated dependencies [cb040ae18]
  - @pnpm/common-cli-options-help@0.3.1
  - @pnpm/cli-utils@0.5.0
  - @pnpm/config@11.14.0

## 2.6.0

### Minor Changes

- c4cc62506: Add '--reverse' flag for reversing the order of package executions during 'recursive run'

### Patch Changes

- Updated dependencies [c4cc62506]
  - @pnpm/config@11.13.0
  - @pnpm/cli-utils@0.4.51

## 2.5.17

### Patch Changes

- Updated dependencies [bff84dbca]
  - @pnpm/config@11.12.1
  - @pnpm/cli-utils@0.4.50

## 2.5.16

### Patch Changes

- @pnpm/cli-utils@0.4.49

## 2.5.15

### Patch Changes

- @pnpm/cli-utils@0.4.48

## 2.5.14

### Patch Changes

- Updated dependencies [9a9bc67d2]
  - @pnpm/lifecycle@9.6.4

## 2.5.13

### Patch Changes

- Updated dependencies [9ad8c27bf]
- Updated dependencies [548f28df9]
- Updated dependencies [548f28df9]
  - @pnpm/types@6.4.0
  - @pnpm/cli-utils@0.4.47
  - @pnpm/config@11.12.0
  - @pnpm/lifecycle@9.6.3
  - @pnpm/sort-packages@1.0.16

## 2.5.12

### Patch Changes

- @pnpm/config@11.11.1
- @pnpm/cli-utils@0.4.46

## 2.5.11

### Patch Changes

- Updated dependencies [f40bc5927]
  - @pnpm/config@11.11.0
  - @pnpm/cli-utils@0.4.45

## 2.5.10

### Patch Changes

- Updated dependencies [425c7547d]
  - @pnpm/config@11.10.2
  - @pnpm/cli-utils@0.4.44

## 2.5.9

### Patch Changes

- Updated dependencies [ea09da716]
  - @pnpm/config@11.10.1
  - @pnpm/cli-utils@0.4.43

## 2.5.8

### Patch Changes

- 9427ab392: `--no-bail` should work with non-recursive `run` commands as well.
- Updated dependencies [1ec47db33]
- Updated dependencies [a8656b42f]
  - @pnpm/common-cli-options-help@0.3.0
  - @pnpm/config@11.10.0
  - @pnpm/cli-utils@0.4.42

## 2.5.7

### Patch Changes

- Updated dependencies [041537bc3]
  - @pnpm/config@11.9.1
  - @pnpm/cli-utils@0.4.41

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
