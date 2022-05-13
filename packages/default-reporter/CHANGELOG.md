# @pnpm/default-reporter

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
