# @pnpm/default-reporter

## 13.1.6

### Patch Changes

- 9bf9f71: When encountering an external dependency using the `catalog:` protocol, a clearer error will be shown. Previously a confusing `ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER` error was thrown. The new error message will explain that the author of the dependency needs to run `pnpm publish` to replace the catalog protocol.
- Updated dependencies [1b03682]
- Updated dependencies [dd00eeb]
- Updated dependencies
  - @pnpm/config@21.6.0
  - @pnpm/types@11.0.0
  - @pnpm/core-loggers@10.0.3
  - @pnpm/render-peer-issues@5.0.4

## 13.1.5

### Patch Changes

- Updated dependencies [7c6c923]
- Updated dependencies [7d10394]
- Updated dependencies [d8eab39]
- Updated dependencies [13e55b2]
- Updated dependencies [04b8363]
  - @pnpm/config@21.5.0
  - @pnpm/types@10.1.1
  - @pnpm/core-loggers@10.0.2
  - @pnpm/render-peer-issues@5.0.3

## 13.1.4

### Patch Changes

- Updated dependencies [47341e5]
  - @pnpm/config@21.4.0

## 13.1.3

### Patch Changes

- Updated dependencies [b7ca13f]
  - @pnpm/config@21.3.0

## 13.1.2

### Patch Changes

- @pnpm/config@21.2.3

## 13.1.1

### Patch Changes

- Updated dependencies [45f4262]
  - @pnpm/types@10.1.0
  - @pnpm/config@21.2.2
  - @pnpm/core-loggers@10.0.1
  - @pnpm/render-peer-issues@5.0.2

## 13.1.0

### Minor Changes

- 524990f: fix(default-reporter): replace deprecated right-pad with String.padEnd

### Patch Changes

- Updated dependencies [a7aef51]
  - @pnpm/error@6.0.1
  - @pnpm/config@21.2.1
  - @pnpm/render-peer-issues@5.0.1

## 13.0.3

### Patch Changes

- 43b6bb7: Print a better error message when `resolution-mode` is set to `time-based` and the registry fails to return the `"time"` field in the package's metadata.

## 13.0.2

### Patch Changes

- Updated dependencies [9719a42]
  - @pnpm/config@21.2.0

## 13.0.1

### Patch Changes

- Updated dependencies [e0f47f4]
  - @pnpm/config@21.1.0

## 13.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.

### Minor Changes

- aa33269: Peer dependency rules should only affect reporting, not data in the lockfile.

### Patch Changes

- Updated dependencies [7733f3a]
- Updated dependencies [aa33269]
- Updated dependencies [3ded840]
- Updated dependencies [43cdd87]
- Updated dependencies [2d9e3b8]
- Updated dependencies [cfa33f1]
- Updated dependencies [e748162]
- Updated dependencies [2b89155]
- Updated dependencies [60839fc]
- Updated dependencies [730929e]
- Updated dependencies [98566d9]
  - @pnpm/types@10.0.0
  - @pnpm/config@21.0.0
  - @pnpm/render-peer-issues@5.0.0
  - @pnpm/error@6.0.0
  - @pnpm/dedupe.issues-renderer@2.0.0
  - @pnpm/core-loggers@10.0.0
  - @pnpm/dedupe.types@2.0.0

## 12.4.13

### Patch Changes

- f12884def: `--aggregate-output` should work on scripts executed from the same project [#7556](https://github.com/pnpm/pnpm/issues/7556).
  - @pnpm/config@20.4.2

## 12.4.12

### Patch Changes

- Updated dependencies [d9564e354]
  - @pnpm/config@20.4.1

## 12.4.11

### Patch Changes

- fac2ed424: `pnpm add a-module-already-in-dev-deps` will show a message to notice the user that the package was not moved to "dependencies" [#926](https://github.com/pnpm/pnpm/issues/926) and fix [#7319](https://github.com/pnpm/pnpm/pull/7319).
- Updated dependencies [c597f72ec]
  - @pnpm/config@20.4.0

## 12.4.10

### Patch Changes

- Updated dependencies [4e71066dd]
- Updated dependencies [4d34684f1]
  - @pnpm/config@20.3.0
  - @pnpm/types@9.4.2
  - @pnpm/core-loggers@9.0.6
  - @pnpm/render-peer-issues@4.0.6

## 12.4.9

### Patch Changes

- Updated dependencies
- Updated dependencies [672c559e4]
  - @pnpm/types@9.4.1
  - @pnpm/config@20.2.0
  - @pnpm/core-loggers@9.0.5
  - @pnpm/render-peer-issues@4.0.5

## 12.4.8

### Patch Changes

- 633c0d6f8: Revert warning about type of dependency.

## 12.4.7

### Patch Changes

- 45bdc79b1: `pnpm add a-module-already-in-dev-deps` will show a message to notice the user that the package was not moved to "dependencies" [#926](https://github.com/pnpm/pnpm/issues/926).

## 12.4.6

### Patch Changes

- @pnpm/config@20.1.2

## 12.4.5

### Patch Changes

- @pnpm/config@20.1.1

## 12.4.4

### Patch Changes

- Updated dependencies [43ce9e4a6]
- Updated dependencies [d6592964f]
  - @pnpm/types@9.4.0
  - @pnpm/config@20.1.0
  - @pnpm/core-loggers@9.0.4
  - @pnpm/render-peer-issues@4.0.4

## 12.4.3

### Patch Changes

- Updated dependencies [ac5abd3ff]
- Updated dependencies [b60bb6cbe]
  - @pnpm/config@20.0.0

## 12.4.2

### Patch Changes

- Updated dependencies [b1dd0ee58]
  - @pnpm/config@19.2.1

## 12.4.1

### Patch Changes

- Updated dependencies [d774a3196]
- Updated dependencies [d774a3196]
- Updated dependencies [832e28826]
  - @pnpm/config@19.2.0
  - @pnpm/types@9.3.0
  - @pnpm/core-loggers@9.0.3
  - @pnpm/render-peer-issues@4.0.3

## 12.4.0

### Minor Changes

- ee328fd25: Add `--hide-reporter-prefix' option for `run` command to hide project name as prefix for lifecycle log outputs of running scripts [#7061](https://github.com/pnpm/pnpm/issues/7061).

### Patch Changes

- Updated dependencies [ee328fd25]
  - @pnpm/config@19.1.0

## 12.3.5

### Patch Changes

- 61b9ca189: Don't print out each deprecated subdependency separately with its deprecation message. Just print out a summary of all the deprecated subdependencies [#6707](https://github.com/pnpm/pnpm/issues/6707).

## 12.3.4

### Patch Changes

- @pnpm/config@19.0.3

## 12.3.3

### Patch Changes

- @pnpm/config@19.0.2

## 12.3.2

### Patch Changes

- @pnpm/config@19.0.1

## 12.3.1

### Patch Changes

- cc785f7e1: Fix a bug causing errors to be printed as `Cannot read properties of undefined (reading 'code')` instead of the underlying reason when using the pnpm store server.
- Updated dependencies [cb8bcc8df]
  - @pnpm/config@19.0.0

## 12.3.0

### Minor Changes

- bc5d3ceda: Add an option to hide the directory prefix in the progress output.
- fe322b678: New option added: hideLifecycleOutput.

### Patch Changes

- f432cb11a: Don't prefix install output for the dlx command.
  - @pnpm/config@18.4.5

## 12.2.9

### Patch Changes

- 8a4dac63c: When showing the download progress of big tarball files, always display the same number of digits after the dot [#6902](https://github.com/pnpm/pnpm/issues/6901).

## 12.2.8

### Patch Changes

- 25396e3c5: When progress is throttled, the last stats should be printed, when importing is done.
- 751c157cd: Don't print "added" stats, when installing with `--lockfile-only`.

## 12.2.7

### Patch Changes

- Updated dependencies [aa2ae8fe2]
  - @pnpm/types@9.2.0
  - @pnpm/config@18.4.4
  - @pnpm/core-loggers@9.0.2
  - @pnpm/render-peer-issues@4.0.2

## 12.2.6

### Patch Changes

- @pnpm/config@18.4.3

## 12.2.5

### Patch Changes

- 100d03b36: When running a script in multiple projects, the script outputs should preserve colours [#2148](https://github.com/pnpm/pnpm/issues/2148).
- Updated dependencies [e2d631217]
  - @pnpm/config@18.4.2

## 12.2.4

### Patch Changes

- @pnpm/config@18.4.1
- @pnpm/error@5.0.2

## 12.2.3

### Patch Changes

- Updated dependencies [a9e0b7cbf]
- Updated dependencies [301b8e2da]
  - @pnpm/types@9.1.0
  - @pnpm/config@18.4.0
  - @pnpm/core-loggers@9.0.1
  - @pnpm/render-peer-issues@4.0.1
  - @pnpm/error@5.0.1

## 12.2.2

### Patch Changes

- Updated dependencies [1de07a4af]
  - @pnpm/config@18.3.2

## 12.2.1

### Patch Changes

- Updated dependencies [2809e89ab]
  - @pnpm/config@18.3.1

## 12.2.0

### Minor Changes

- 6850bb135: Report errors from pnpm dedupe --check

### Patch Changes

- 31ca5a218: Don't print empty sections in the summary, when results are filtered.
- c0760128d: bump semver to 7.4.0
- Updated dependencies [32f8e08c6]
- Updated dependencies [6850bb135]
- Updated dependencies [6850bb135]
  - @pnpm/config@18.3.0
  - @pnpm/dedupe.issues-renderer@1.0.0
  - @pnpm/dedupe.types@1.0.0

## 12.1.0

### Minor Changes

- 6cfaf31a1: In order to filter out packages from the installation summary, a filter function may be passed to the reporter: filterPkgsDiff.

### Patch Changes

- Updated dependencies [fc8780ca9]
  - @pnpm/config@18.2.0

## 12.0.4

### Patch Changes

- af3e5559d: Should report error summary as expected.
  - @pnpm/config@18.1.1

## 12.0.3

### Patch Changes

- Updated dependencies [e2cb4b63d]
- Updated dependencies [cd6ce11f0]
  - @pnpm/config@18.1.0

## 12.0.2

### Patch Changes

- @pnpm/config@18.0.2

## 12.0.1

### Patch Changes

- @pnpm/config@18.0.1

## 12.0.0

### Major Changes

- eceaa8b8b: Node.js 14 support dropped.

### Patch Changes

- Updated dependencies [47e45d717]
- Updated dependencies [47e45d717]
- Updated dependencies [158d8cf22]
- Updated dependencies [eceaa8b8b]
- Updated dependencies [8e35c21d1]
- Updated dependencies [47e45d717]
- Updated dependencies [47e45d717]
- Updated dependencies [113f0ae26]
  - @pnpm/config@18.0.0
  - @pnpm/render-peer-issues@4.0.0
  - @pnpm/core-loggers@9.0.0
  - @pnpm/error@5.0.0
  - @pnpm/types@9.0.0

## 11.0.42

### Patch Changes

- @pnpm/config@17.0.2

## 11.0.41

### Patch Changes

- Updated dependencies [b38d711f3]
  - @pnpm/config@17.0.1

## 11.0.40

### Patch Changes

- Updated dependencies [e505b58e3]
  - @pnpm/config@17.0.0

## 11.0.39

### Patch Changes

- @pnpm/config@16.7.2

## 11.0.38

### Patch Changes

- @pnpm/config@16.7.1

## 11.0.37

### Patch Changes

- Updated dependencies [5c31fa8be]
  - @pnpm/config@16.7.0

## 11.0.36

### Patch Changes

- @pnpm/config@16.6.4

## 11.0.35

### Patch Changes

- @pnpm/config@16.6.3

## 11.0.34

### Patch Changes

- @pnpm/config@16.6.2

## 11.0.33

### Patch Changes

- @pnpm/config@16.6.1

## 11.0.32

### Patch Changes

- Updated dependencies [59ee53678]
  - @pnpm/config@16.6.0

## 11.0.31

### Patch Changes

- @pnpm/config@16.5.5

## 11.0.30

### Patch Changes

- @pnpm/config@16.5.4

## 11.0.29

### Patch Changes

- @pnpm/config@16.5.3

## 11.0.28

### Patch Changes

- @pnpm/config@16.5.2

## 11.0.27

### Patch Changes

- @pnpm/config@16.5.1

## 11.0.26

### Patch Changes

- Updated dependencies [28b47a156]
  - @pnpm/config@16.5.0

## 11.0.25

### Patch Changes

- @pnpm/config@16.4.3

## 11.0.24

### Patch Changes

- @pnpm/config@16.4.2

## 11.0.23

### Patch Changes

- @pnpm/config@16.4.1

## 11.0.22

### Patch Changes

- Updated dependencies [3ebce5db7]
  - @pnpm/config@16.4.0
  - @pnpm/error@4.0.1

## 11.0.21

### Patch Changes

- Updated dependencies [1fad508b0]
  - @pnpm/config@16.3.0

## 11.0.20

### Patch Changes

- ec97a3105: Report to the console when a git-hosted dependency is built [#5847](https://github.com/pnpm/pnpm/pull/5847).
  - @pnpm/config@16.2.2

## 11.0.19

### Patch Changes

- Updated dependencies [d71dbf230]
  - @pnpm/config@16.2.1

## 11.0.18

### Patch Changes

- 0048e0e64: Fix the command in the hint about how to update the store location globally.
- Updated dependencies [841f52e70]
  - @pnpm/config@16.2.0

## 11.0.17

### Patch Changes

- Updated dependencies [b77651d14]
  - @pnpm/types@8.10.0
  - @pnpm/config@16.1.11
  - @pnpm/core-loggers@8.0.3
  - @pnpm/render-peer-issues@3.0.3

## 11.0.16

### Patch Changes

- @pnpm/config@16.1.10

## 11.0.15

### Patch Changes

- @pnpm/config@16.1.9

## 11.0.14

### Patch Changes

- 3f644a514: The update notifier should suggest using the standalone script, when pnpm was installed using a standalone script.
  - @pnpm/config@16.1.8

## 11.0.13

### Patch Changes

- Updated dependencies [a9d59d8bc]
  - @pnpm/config@16.1.7

## 11.0.12

### Patch Changes

- @pnpm/config@16.1.6

## 11.0.11

### Patch Changes

- @pnpm/config@16.1.5

## 11.0.10

### Patch Changes

- @pnpm/config@16.1.4

## 11.0.9

### Patch Changes

- @pnpm/config@16.1.3

## 11.0.8

### Patch Changes

- @pnpm/config@16.1.2

## 11.0.7

### Patch Changes

- @pnpm/config@16.1.1

## 11.0.6

### Patch Changes

- Updated dependencies [3dab7f83c]
  - @pnpm/config@16.1.0

## 11.0.5

### Patch Changes

- a4c58d424: The reporter should not crash when the CLI process is kill during lifecycle scripts execution [#5588](https://github.com/pnpm/pnpm/pull/5588).
- Updated dependencies [702e847c1]
  - @pnpm/types@8.9.0
  - @pnpm/config@16.0.5
  - @pnpm/core-loggers@8.0.2
  - @pnpm/render-peer-issues@3.0.2

## 11.0.4

### Patch Changes

- @pnpm/config@16.0.4

## 11.0.3

### Patch Changes

- 0018cd03e: Don't print context information when running install for the `pnpm dlx` command.
- Updated dependencies [aacb83f73]
- Updated dependencies [a14ad09e6]
  - @pnpm/config@16.0.3

## 11.0.2

### Patch Changes

- Updated dependencies [bea0acdfc]
  - @pnpm/config@16.0.2

## 11.0.1

### Patch Changes

- Updated dependencies [e7fd8a84c]
- Updated dependencies [844e82f3a]
  - @pnpm/config@16.0.1
  - @pnpm/types@8.8.0
  - @pnpm/core-loggers@8.0.1
  - @pnpm/render-peer-issues@3.0.1

## 11.0.0

### Major Changes

- 043d988fc: Breaking change to the API. Defaul export is not used.
- f884689e0: Require `@pnpm/logger` v5.

### Patch Changes

- Updated dependencies [043d988fc]
- Updated dependencies [1d0fd82fd]
- Updated dependencies [645384bfd]
- Updated dependencies [f884689e0]
- Updated dependencies [3c117996e]
  - @pnpm/config@16.0.0
  - @pnpm/error@4.0.0
  - @pnpm/core-loggers@8.0.0
  - @pnpm/render-peer-issues@3.0.0

## 10.1.1

### Patch Changes

- @pnpm/config@15.10.12

## 10.1.0

### Minor Changes

- 3ae888c28: Show execution time on `install`, `update`, `add`, and `remove` [#1021](https://github.com/pnpm/pnpm/issues/1021).

### Patch Changes

- Updated dependencies [3ae888c28]
  - @pnpm/core-loggers@7.1.0
  - @pnpm/config@15.10.11

## 10.0.1

### Patch Changes

- e8a631bf0: When a direct dependency fails to resolve, print the path to the project directory in the error message.
- Updated dependencies [e8a631bf0]
  - @pnpm/error@3.1.0
  - @pnpm/config@15.10.10

## 10.0.0

### Major Changes

- 51566e34b: Accept an array of hooks.

### Patch Changes

- Updated dependencies [d665f3ff7]
  - @pnpm/types@8.7.0
  - @pnpm/config@15.10.9
  - @pnpm/core-loggers@7.0.8
  - @pnpm/render-peer-issues@2.1.2

## 9.1.28

### Patch Changes

- @pnpm/config@15.10.8

## 9.1.27

### Patch Changes

- @pnpm/config@15.10.7

## 9.1.26

### Patch Changes

- Updated dependencies [156cc1ef6]
  - @pnpm/types@8.6.0
  - @pnpm/config@15.10.6
  - @pnpm/core-loggers@7.0.7
  - @pnpm/render-peer-issues@2.1.1

## 9.1.25

### Patch Changes

- @pnpm/config@15.10.5

## 9.1.24

### Patch Changes

- 728c0cdf6: When an error happens during installation of a subdependency, print some context information in order to be able to locate that subdependency. Print the exact chain of packages that led to the problematic dependency.
  - @pnpm/config@15.10.4

## 9.1.23

### Patch Changes

- @pnpm/config@15.10.3

## 9.1.22

### Patch Changes

- @pnpm/config@15.10.2

## 9.1.21

### Patch Changes

- @pnpm/config@15.10.1

## 9.1.20

### Patch Changes

- Updated dependencies [2aa22e4b1]
  - @pnpm/config@15.10.0

## 9.1.19

### Patch Changes

- @pnpm/config@15.9.4

## 9.1.18

### Patch Changes

- @pnpm/config@15.9.3

## 9.1.17

### Patch Changes

- @pnpm/config@15.9.2

## 9.1.16

### Patch Changes

- @pnpm/config@15.9.1

## 9.1.15

### Patch Changes

- 39c040127: upgrade various dependencies
- 8103f92bd: Use a patched version of ramda to fix deprecation warnings on Node.js 16. Related issue: https://github.com/ramda/ramda/pull/3270
- Updated dependencies [43cd6aaca]
- Updated dependencies [8103f92bd]
- Updated dependencies [65c4260de]
- Updated dependencies [29a81598a]
- Updated dependencies [c990a409f]
  - @pnpm/config@15.9.0
  - @pnpm/render-peer-issues@2.1.0

## 9.1.14

### Patch Changes

- Updated dependencies [c90798461]
- Updated dependencies [34121d753]
  - @pnpm/types@8.5.0
  - @pnpm/config@15.8.1
  - @pnpm/core-loggers@7.0.6
  - @pnpm/render-peer-issues@2.0.6

## 9.1.13

### Patch Changes

- Updated dependencies [cac34ad69]
- Updated dependencies [99019e071]
  - @pnpm/config@15.8.0

## 9.1.12

### Patch Changes

- @pnpm/config@15.7.1

## 9.1.11

### Patch Changes

- Updated dependencies [4fa1091c8]
  - @pnpm/config@15.7.0

## 9.1.10

### Patch Changes

- Updated dependencies [7334b347b]
  - @pnpm/config@15.6.1

## 9.1.9

### Patch Changes

- Updated dependencies [28f000509]
- Updated dependencies [406656f80]
  - @pnpm/config@15.6.0

## 9.1.8

### Patch Changes

- c71215041: Do not print a package with unchanged version in the installation summary.
  - @pnpm/config@15.5.2

## 9.1.7

### Patch Changes

- 5f643f23b: Update ramda to v0.28.
- Updated dependencies [5f643f23b]
  - @pnpm/config@15.5.1

## 9.1.6

### Patch Changes

- Updated dependencies [f48d46ef6]
  - @pnpm/config@15.5.0

## 9.1.5

### Patch Changes

- Updated dependencies [8e5b77ef6]
  - @pnpm/types@8.4.0
  - @pnpm/config@15.4.1
  - @pnpm/core-loggers@7.0.5
  - @pnpm/render-peer-issues@2.0.5

## 9.1.4

### Patch Changes

- Updated dependencies [2a34b21ce]
- Updated dependencies [47b5e45dd]
  - @pnpm/types@8.3.0
  - @pnpm/config@15.4.0
  - @pnpm/core-loggers@7.0.4
  - @pnpm/render-peer-issues@2.0.4

## 9.1.3

### Patch Changes

- Updated dependencies [fb5bbfd7a]
- Updated dependencies [56cf04cb3]
  - @pnpm/types@8.2.0
  - @pnpm/config@15.3.0
  - @pnpm/core-loggers@7.0.3
  - @pnpm/render-peer-issues@2.0.3

## 9.1.2

### Patch Changes

- Updated dependencies [25798aad1]
  - @pnpm/config@15.2.1

## 9.1.1

### Patch Changes

- 9b7941c81: Add better hints to the peer dependency issue errors.
- Updated dependencies [4d39e4a0c]
- Updated dependencies [bc80631d3]
- Updated dependencies [d5730ba81]
  - @pnpm/types@8.1.0
  - @pnpm/config@15.2.0
  - @pnpm/core-loggers@7.0.2
  - @pnpm/render-peer-issues@2.0.2

## 9.1.0

### Minor Changes

- 2493b8ef3: Suggest to update using Corepack when pnpm was installed via Corepack.

### Patch Changes

- @pnpm/config@15.1.4

## 9.0.8

### Patch Changes

- Updated dependencies [ae2f845c5]
  - @pnpm/config@15.1.4

## 9.0.7

### Patch Changes

- Updated dependencies [05159665d]
  - @pnpm/config@15.1.3

## 9.0.6

### Patch Changes

- 190f0b331: Add hints to the peer dependencies error.

## 9.0.5

### Patch Changes

- Updated dependencies [af22c6c4f]
  - @pnpm/config@15.1.2

## 9.0.4

### Patch Changes

- 3b98e43a9: Do not report request retry warnings when loglevel is set to `error` [#4669](https://github.com/pnpm/pnpm/issues/4669).
  - @pnpm/config@15.1.1

## 9.0.3

### Patch Changes

- Updated dependencies [18ba5e2c0]
  - @pnpm/types@8.0.1
  - @pnpm/config@15.1.1
  - @pnpm/core-loggers@7.0.1
  - @pnpm/render-peer-issues@2.0.1

## 9.0.2

### Patch Changes

- Updated dependencies [e05dcc48a]
  - @pnpm/config@15.1.0

## 9.0.1

### Patch Changes

- e94149987: Hide "WARN deprecated" messages on loglevel error [#4507](https://github.com/pnpm/pnpm/pull/4507)

  Don't show the progress bar when loglevel is set to warn or error.

- Updated dependencies [8dac029ef]
- Updated dependencies [72b79f55a]
- Updated dependencies [546e644e9]
- Updated dependencies [c6463b9fd]
- Updated dependencies [4bed585e2]
- Updated dependencies [8fa95fd86]
  - @pnpm/config@15.0.0
  - @pnpm/error@3.0.1

## 9.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [516859178]
- Updated dependencies [d504dc380]
- Updated dependencies [73d71a2d5]
- Updated dependencies [fa656992c]
- Updated dependencies [542014839]
- Updated dependencies [585e9ca9e]
  - @pnpm/config@14.0.0
  - @pnpm/types@8.0.0
  - @pnpm/core-loggers@7.0.0
  - @pnpm/error@3.0.0
  - @pnpm/render-peer-issues@2.0.0

## 8.5.13

### Patch Changes

- Updated dependencies [70ba51da9]
  - @pnpm/error@2.1.0
  - @pnpm/config@13.13.2

## 8.5.12

### Patch Changes

- 5f00eb0e0: When some dependency types are skipped, let the user know via the installation summary.
- Updated dependencies [b138d048c]
  - @pnpm/types@7.10.0
  - @pnpm/config@13.13.1
  - @pnpm/core-loggers@6.1.4
  - @pnpm/render-peer-issues@1.1.2

## 8.5.11

### Patch Changes

- Updated dependencies [334e5340a]
  - @pnpm/config@13.13.0

## 8.5.10

### Patch Changes

- Updated dependencies [b7566b979]
  - @pnpm/config@13.12.0

## 8.5.9

### Patch Changes

- Updated dependencies [fff0e4493]
  - @pnpm/config@13.11.0

## 8.5.8

### Patch Changes

- a1ffef5ca: Print warnings about deprecated subdependencies [#4227](https://github.com/pnpm/pnpm/issues/4227).

## 8.5.7

### Patch Changes

- Updated dependencies [e76151f66]
- Updated dependencies [26cd01b88]
  - @pnpm/config@13.10.0
  - @pnpm/types@7.9.0
  - @pnpm/core-loggers@6.1.3
  - @pnpm/render-peer-issues@1.1.1

## 8.5.6

### Patch Changes

- ea24c69fe: `@pnpm/logger` should be a peer dependency.

## 8.5.5

### Patch Changes

- Updated dependencies [8fe8f5e55]
  - @pnpm/config@13.9.0

## 8.5.4

### Patch Changes

- Updated dependencies [732d4962f]
- Updated dependencies [a6cf11cb7]
  - @pnpm/config@13.8.0

## 8.5.3

### Patch Changes

- Updated dependencies [b5734a4a7]
- Updated dependencies [b5734a4a7]
  - @pnpm/render-peer-issues@1.1.0
  - @pnpm/types@7.8.0
  - @pnpm/config@13.7.2
  - @pnpm/core-loggers@6.1.2

## 8.5.2

### Patch Changes

- Updated dependencies [6058f76cd]
  - @pnpm/render-peer-issues@1.0.2

## 8.5.1

### Patch Changes

- Updated dependencies [6493e0c93]
- Updated dependencies [a087f339e]
  - @pnpm/types@7.7.1
  - @pnpm/render-peer-issues@1.0.1
  - @pnpm/config@13.7.1
  - @pnpm/core-loggers@6.1.1

## 8.5.0

### Minor Changes

- ba9b2eba1: Add peerDependencyIssuesLogger.
- 927c4a089: A new option `--aggregate-output` for `append-only` reporter is added. It aggregates lifecycle logs output for each command that is run in parallel, and only prints command logs when command is finished.

  Related discussion: [#4070](https://github.com/pnpm/pnpm/discussions/4070).

### Patch Changes

- Updated dependencies [ba9b2eba1]
- Updated dependencies [30bfca967]
- Updated dependencies [927c4a089]
- Updated dependencies [10a4bd4db]
- Updated dependencies [ba9b2eba1]
- Updated dependencies [ba9b2eba1]
  - @pnpm/core-loggers@6.1.0
  - @pnpm/config@13.7.0
  - @pnpm/render-peer-issues@1.0.0
  - @pnpm/types@7.7.0

## 8.4.2

### Patch Changes

- Updated dependencies [46aaf7108]
  - @pnpm/config@13.6.1

## 8.4.1

### Patch Changes

- Updated dependencies [8a99a01ff]
  - @pnpm/config@13.6.0

## 8.4.0

### Minor Changes

- 597a28e3c: The default reporter returns an unsubscribe function to stop reporting.

## 8.3.8

### Patch Changes

- Updated dependencies [a7ff2d5ce]
  - @pnpm/config@13.5.1

## 8.3.7

### Patch Changes

- Updated dependencies [002778559]
  - @pnpm/config@13.5.0

## 8.3.6

### Patch Changes

- Updated dependencies [302ae4f6f]
  - @pnpm/types@7.6.0
  - @pnpm/config@13.4.2
  - @pnpm/core-loggers@6.0.6

## 8.3.5

### Patch Changes

- Updated dependencies [4ab87844a]
  - @pnpm/types@7.5.0
  - @pnpm/config@13.4.1
  - @pnpm/core-loggers@6.0.5

## 8.3.4

### Patch Changes

- 7a021932f: Update stacktracey to v2.
- Updated dependencies [b6d74c545]
  - @pnpm/config@13.4.0

## 8.3.3

### Patch Changes

- Updated dependencies [bd7bcdbe8]
  - @pnpm/config@13.3.0

## 8.3.2

### Patch Changes

- Updated dependencies [5ee3b2dc7]
  - @pnpm/config@13.2.0

## 8.3.1

### Patch Changes

- cd597bdf9: Suggest `pnpm install --force` to refetch modified packages.

## 8.3.0

### Minor Changes

- ef9d2719a: New hook supported for filtering out info and warning logs: `filterLog(log) => boolean`.

### Patch Changes

- Updated dependencies [4027a3c69]
  - @pnpm/config@13.1.0

## 8.2.3

### Patch Changes

- Updated dependencies [fe5688dc0]
- Updated dependencies [c7081cbb4]
- Updated dependencies [c7081cbb4]
  - @pnpm/config@13.0.0

## 8.2.2

### Patch Changes

- Updated dependencies [d62259d67]
  - @pnpm/config@12.6.0

## 8.2.1

### Patch Changes

- Updated dependencies [6681fdcbc]
  - @pnpm/config@12.5.0

## 8.2.0

### Minor Changes

- e0aa55140: Print error codes in error messages.

## 8.1.14

### Patch Changes

- Updated dependencies [ede519190]
  - @pnpm/config@12.4.9

## 8.1.13

### Patch Changes

- @pnpm/config@12.4.8

## 8.1.12

### Patch Changes

- Updated dependencies [655af55ba]
  - @pnpm/config@12.4.7

## 8.1.11

### Patch Changes

- Updated dependencies [3fb74c618]
  - @pnpm/config@12.4.6

## 8.1.10

### Patch Changes

- Updated dependencies [051296a16]
  - @pnpm/config@12.4.5

## 8.1.9

### Patch Changes

- Updated dependencies [af8b5716e]
  - @pnpm/config@12.4.4

## 8.1.8

### Patch Changes

- Updated dependencies [b734b45ea]
  - @pnpm/types@7.4.0
  - @pnpm/config@12.4.3
  - @pnpm/core-loggers@6.0.4

## 8.1.7

### Patch Changes

- Updated dependencies [73c1f802e]
  - @pnpm/config@12.4.2

## 8.1.6

### Patch Changes

- 67c6a67f9: Do not collapse warnings when reporting is append-only.

## 8.1.5

### Patch Changes

- Updated dependencies [2264bfdf4]
  - @pnpm/config@12.4.1

## 8.1.4

### Patch Changes

- Updated dependencies [25f6968d4]
- Updated dependencies [5aaf3e3fa]
  - @pnpm/config@12.4.0

## 8.1.3

### Patch Changes

- Updated dependencies [8e76690f4]
  - @pnpm/types@7.3.0
  - @pnpm/config@12.3.3
  - @pnpm/core-loggers@6.0.3

## 8.1.2

### Patch Changes

- Updated dependencies [724c5abd8]
  - @pnpm/types@7.2.0
  - @pnpm/config@12.3.2
  - @pnpm/core-loggers@6.0.2

## 8.1.1

### Patch Changes

- a1a03d145: Import only the required functions from ramda.
- Updated dependencies [a1a03d145]
  - @pnpm/config@12.3.1

## 8.1.0

### Minor Changes

- c2a71e4fd: New CLI option added: `use-stderr`. When set, all the output is written to stderr.

### Patch Changes

- Updated dependencies [84ec82e05]
- Updated dependencies [c2a71e4fd]
- Updated dependencies [84ec82e05]
  - @pnpm/config@12.3.0

## 8.0.3

### Patch Changes

- e4a981c0c: Update rxjs.

## 8.0.2

### Patch Changes

- Updated dependencies [05baaa6e7]
- Updated dependencies [dfdf669e6]
- Updated dependencies [97c64bae4]
  - @pnpm/config@12.2.0
  - @pnpm/types@7.1.0
  - @pnpm/core-loggers@6.0.1

## 8.0.1

### Patch Changes

- Updated dependencies [ba5231ccf]
  - @pnpm/config@12.1.0

## 8.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Minor Changes

- 90487a3a8: Print a notification about newer version of the CLI.

### Patch Changes

- Updated dependencies [97b986fbc]
- Updated dependencies [90487a3a8]
- Updated dependencies [78470a32d]
- Updated dependencies [aed712455]
- Updated dependencies [aed712455]
  - @pnpm/config@12.0.0
  - @pnpm/core-loggers@6.0.0
  - @pnpm/error@2.0.0
  - @pnpm/types@7.0.0

## 7.10.7

### Patch Changes

- Updated dependencies [4f1ce907a]
  - @pnpm/config@11.14.2

## 7.10.6

### Patch Changes

- Updated dependencies [4b3852c39]
  - @pnpm/config@11.14.1

## 7.10.5

### Patch Changes

- @pnpm/config@11.14.0

## 7.10.4

### Patch Changes

- Updated dependencies [cb040ae18]
  - @pnpm/config@11.14.0

## 7.10.3

### Patch Changes

- Updated dependencies [c4cc62506]
  - @pnpm/config@11.13.0

## 7.10.2

### Patch Changes

- Updated dependencies [bff84dbca]
  - @pnpm/config@11.12.1

## 7.10.1

### Patch Changes

- 4420f9f4e: Substitute pretty-time with pretty-ms.

## 7.10.0

### Minor Changes

- 548f28df9: Export `formatWarn(warningMessage: string): string`.

### Patch Changes

- Updated dependencies [9ad8c27bf]
- Updated dependencies [548f28df9]
  - @pnpm/types@6.4.0
  - @pnpm/config@11.12.0
  - @pnpm/core-loggers@5.0.3

## 7.9.16

### Patch Changes

- @pnpm/config@11.11.1

## 7.9.15

### Patch Changes

- Updated dependencies [f40bc5927]
  - @pnpm/config@11.11.0

## 7.9.14

### Patch Changes

- Updated dependencies [425c7547d]
  - @pnpm/config@11.10.2

## 7.9.13

### Patch Changes

- Updated dependencies [ea09da716]
  - @pnpm/config@11.10.1

## 7.9.12

### Patch Changes

- Updated dependencies [a8656b42f]
  - @pnpm/config@11.10.0

## 7.9.11

### Patch Changes

- 8c21dc57f: Normalize path in context reporting.
- Updated dependencies [041537bc3]
  - @pnpm/config@11.9.1

## 7.9.10

### Patch Changes

- Updated dependencies [8698a7060]
  - @pnpm/config@11.9.0

## 7.9.9

### Patch Changes

- Updated dependencies [fcc1c7100]
  - @pnpm/config@11.8.0

## 7.9.8

### Patch Changes

- Updated dependencies [0c5f1bcc9]
  - @pnpm/error@1.4.0
  - @pnpm/config@11.7.2

## 7.9.7

### Patch Changes

- Updated dependencies [b5d694e7f]
  - @pnpm/types@6.3.1
  - @pnpm/config@11.7.1
  - @pnpm/core-loggers@5.0.2

## 7.9.6

### Patch Changes

- Updated dependencies [50b360ec1]
  - @pnpm/config@11.7.0

## 7.9.5

### Patch Changes

- Updated dependencies [d54043ee4]
  - @pnpm/types@6.3.0
  - @pnpm/config@11.6.1
  - @pnpm/core-loggers@5.0.1

## 7.9.4

### Patch Changes

- Updated dependencies [f591fdeeb]
  - @pnpm/config@11.6.0

## 7.9.3

### Patch Changes

- Updated dependencies [74914c178]
  - @pnpm/config@11.5.0

## 7.9.2

### Patch Changes

- Updated dependencies [23cf3c88b]
  - @pnpm/config@11.4.0

## 7.9.1

### Patch Changes

- 3b8e3b6b1: Always print the final progress stats.
- Updated dependencies [767212f4e]
- Updated dependencies [092f8dd83]
  - @pnpm/config@11.3.0

## 7.9.0

### Minor Changes

- 663afd68e: Scope is not reported when the scope is only one project.
- 86cd72de3: Show the progress of adding packages to the virtual store.

### Patch Changes

- Updated dependencies [86cd72de3]
  - @pnpm/core-loggers@5.0.0

## 7.8.0

### Minor Changes

- 09b42d3ab: Use RxJS instead of "most".

## 7.7.0

### Minor Changes

- af8361946: Sometimes, when installing new dependencies that rely on many peer dependencies, or when running installation on a huge monorepo, there will be hundreds or thousands of warnings. Printing many messages to the terminal is expensive and reduces speed, so pnpm will only print a few warnings and report the total number of the unprinted warnings.

## 7.6.4

### Patch Changes

- Updated dependencies [75a36deba]
- Updated dependencies [9f1a29ff9]
  - @pnpm/error@1.3.1
  - @pnpm/config@11.2.7

## 7.6.3

### Patch Changes

- 13c332e69: Fixes a regression published in pnpm v5.5.3 as a result of nullish coalescing refactoring.

## 7.6.2

### Patch Changes

- Updated dependencies [ac0d3e122]
  - @pnpm/config@11.2.6

## 7.6.1

### Patch Changes

- Updated dependencies [972864e0d]
  - @pnpm/config@11.2.5

## 7.6.0

### Minor Changes

- 6d480dd7a: Print the authorization settings (with hidden private info), when an authorization error happens during fetch.

### Patch Changes

- Updated dependencies [6d480dd7a]
  - @pnpm/error@1.3.0
  - @pnpm/config@11.2.4

## 7.5.4

### Patch Changes

- Updated dependencies [13c18e397]
  - @pnpm/config@11.2.3

## 7.5.3

### Patch Changes

- Updated dependencies [3f6d35997]
  - @pnpm/config@11.2.2

## 7.5.2

### Patch Changes

- Updated dependencies [a2ef8084f]
  - @pnpm/config@11.2.1

## 7.5.1

### Patch Changes

- Updated dependencies [ad69677a7]
  - @pnpm/config@11.2.0

## 7.5.0

### Minor Changes

- 9a908bc07: Print info after install about hardlinked/copied packages in `node_modules/.pnpm`

### Patch Changes

- Updated dependencies [9a908bc07]
- Updated dependencies [9a908bc07]
  - @pnpm/core-loggers@4.2.0

## 7.4.7

### Patch Changes

- Updated dependencies [65b4d07ca]
- Updated dependencies [ab3b8f51d]
  - @pnpm/config@11.1.0

## 7.4.6

### Patch Changes

- @pnpm/config@11.0.1

## 7.4.5

### Patch Changes

- Updated dependencies [71aeb9a38]
- Updated dependencies [915828b46]
  - @pnpm/config@11.0.0

## 7.4.4

### Patch Changes

- @pnpm/config@10.0.1

## 7.4.3

### Patch Changes

- 220896511: Remove common-tags from dependencies.
- Updated dependencies [db17f6f7b]
- Updated dependencies [1146b76d2]
- Updated dependencies [db17f6f7b]
  - @pnpm/config@10.0.0
  - @pnpm/types@6.2.0
  - @pnpm/core-loggers@4.1.2

## 7.4.2

### Patch Changes

- Updated dependencies [71a8c8ce3]
- Updated dependencies [71a8c8ce3]
  - @pnpm/types@6.1.0
  - @pnpm/config@9.2.0
  - @pnpm/core-loggers@4.1.1

## 7.4.1

### Patch Changes

- e934b1a48: Update chalk to v4.1.0.

## 7.4.0

### Minor Changes

- 2ebb7af33: New reporter added for request retries.

### Patch Changes

- Updated dependencies [2ebb7af33]
  - @pnpm/core-loggers@4.1.0

## 7.3.0

### Minor Changes

- eb82084e1: Color the different output prefixes differently.
- ffddf34a8: Add new reporting option: `streamLifecycleOutput`. When `true`, the output from child processes is printed immediately and is never collapsed.

### Patch Changes

- Updated dependencies [ffddf34a8]
  - @pnpm/config@9.1.0

## 7.2.5

### Patch Changes

- Updated dependencies [242cf8737]
- Updated dependencies [da091c711]
- Updated dependencies [e11019b89]
- Updated dependencies [802d145fc]
- Updated dependencies [45fdcfde2]
  - @pnpm/config@9.0.0
  - @pnpm/types@6.0.0
  - @pnpm/core-loggers@4.0.2
  - @pnpm/error@1.2.1

## 7.2.5-alpha.2

### Patch Changes

- Updated dependencies [242cf8737]
- Updated dependencies [45fdcfde2]
  - @pnpm/config@9.0.0-alpha.2

## 7.2.5-alpha.1

### Patch Changes

- Updated dependencies [da091c71]
  - @pnpm/types@6.0.0-alpha.0
  - @pnpm/config@8.3.1-alpha.1
  - @pnpm/core-loggers@4.0.2-alpha.0

## 7.2.5-alpha.0

### Patch Changes

- @pnpm/config@8.3.1-alpha.0

## 7.2.4

### Patch Changes

- 907c63a48: Global warnings are reported.
