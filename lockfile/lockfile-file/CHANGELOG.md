# @pnpm/lockfile-file

## 9.1.2

### Patch Changes

- Updated dependencies [dd00eeb]
- Updated dependencies
  - @pnpm/types@11.0.0
  - @pnpm/lockfile-utils@11.0.3
  - @pnpm/git-resolver@9.0.4
  - @pnpm/lockfile-types@7.1.2
  - @pnpm/merge-lockfile-changes@6.0.4
  - @pnpm/dependency-path@5.1.2

## 9.1.1

### Patch Changes

- Updated dependencies [13e55b2]
  - @pnpm/types@10.1.1
  - @pnpm/lockfile-types@7.1.1
  - @pnpm/lockfile-utils@11.0.2
  - @pnpm/merge-lockfile-changes@6.0.3
  - @pnpm/dependency-path@5.1.1
  - @pnpm/git-resolver@9.0.3

## 9.1.0

### Minor Changes

- 47341e5: **Semi-breaking.** Dependency key names in the lockfile are shortened if they are longer than 1000 characters. We don't expect this change to affect many users. Affected users most probably can't run install successfully at the moment. This change is required to fix some edge cases in which installation fails with an out-of-memory error or "Invalid string length (RangeError: Invalid string length)" error. The max allowed length of the dependency key can be controlled with the `peers-suffix-max-length` setting [#8177](https://github.com/pnpm/pnpm/pull/8177).

### Patch Changes

- Updated dependencies [47341e5]
  - @pnpm/dependency-path@5.1.0
  - @pnpm/lockfile-types@7.1.0
  - @pnpm/lockfile-utils@11.0.1
  - @pnpm/merge-lockfile-changes@6.0.2

## 9.0.6

### Patch Changes

- Updated dependencies [45f4262]
- Updated dependencies
  - @pnpm/types@10.1.0
  - @pnpm/lockfile-types@7.0.0
  - @pnpm/lockfile-utils@11.0.0
  - @pnpm/dependency-path@5.0.0
  - @pnpm/merge-lockfile-changes@6.0.1
  - @pnpm/git-resolver@9.0.2

## 9.0.5

### Patch Changes

- Updated dependencies [a7aef51]
  - @pnpm/error@6.0.1

## 9.0.4

### Patch Changes

- Updated dependencies [7a0536e]
  - @pnpm/lockfile-utils@10.1.1

## 9.0.3

### Patch Changes

- Updated dependencies [9719a42]
  - @pnpm/dependency-path@4.0.0
  - @pnpm/lockfile-utils@10.1.0

## 9.0.2

### Patch Changes

- c969f37: Lockfiles that have git-hosted dependencies specified should be correctly converted to the new lockfile format [#7990](https://github.com/pnpm/pnpm/issues/7990).
- Updated dependencies [c969f37]
  - @pnpm/git-resolver@9.0.1

## 9.0.1

### Patch Changes

- 2cbf7b7: Lockfiles with local or git-hosted dependencies are now successfully converted to the new lockfile format [#7955](https://github.com/pnpm/pnpm/issues/7955).
- 6b6ca69: The lockfile should be saved in the new format even if it is up-to-date.

## 9.0.0

### Major Changes

- 43cdd87: Node.js v16 support dropped. Use at least Node.js v18.12.
- d381a60: Support for lockfile v5 is dropped. Use pnpm v8 to convert lockfile v5 to lockfile v6 [#7470](https://github.com/pnpm/pnpm/pull/7470).

### Minor Changes

- 086b69c: The checksum of the `.pnpmfile.cjs` is saved into the lockfile. If the pnpmfile gets modified, the lockfile is reanalyzed to apply the changes [#7662](https://github.com/pnpm/pnpm/pull/7662).
- 730929e: Add a field named `ignoredOptionalDependencies`. This is an array of strings. If an optional dependency has its name included in this array, it will be skipped.

### Patch Changes

- f67ad31: Never wrap lines in the lockfile.
- Updated dependencies [7733f3a]
- Updated dependencies [3ded840]
- Updated dependencies [cdd8365]
- Updated dependencies [c692f80]
- Updated dependencies [89b396b]
- Updated dependencies [43cdd87]
- Updated dependencies [086b69c]
- Updated dependencies [d381a60]
- Updated dependencies [27a96a8]
- Updated dependencies [730929e]
- Updated dependencies [98a1266]
  - @pnpm/types@10.0.0
  - @pnpm/error@6.0.0
  - @pnpm/dependency-path@3.0.0
  - @pnpm/lockfile-utils@10.0.0
  - @pnpm/constants@8.0.0
  - @pnpm/merge-lockfile-changes@6.0.0
  - @pnpm/lockfile-types@6.0.0
  - @pnpm/git-utils@2.0.0

## 8.1.6

### Patch Changes

- Updated dependencies [4d34684f1]
  - @pnpm/lockfile-types@5.1.5
  - @pnpm/types@9.4.2
  - @pnpm/merge-lockfile-changes@5.0.7
  - @pnpm/dependency-path@2.1.7

## 8.1.5

### Patch Changes

- Updated dependencies
  - @pnpm/lockfile-types@5.1.4
  - @pnpm/types@9.4.1
  - @pnpm/merge-lockfile-changes@5.0.6
  - @pnpm/dependency-path@2.1.6

## 8.1.4

### Patch Changes

- Updated dependencies [43ce9e4a6]
  - @pnpm/types@9.4.0
  - @pnpm/lockfile-types@5.1.3
  - @pnpm/dependency-path@2.1.5
  - @pnpm/merge-lockfile-changes@5.0.5

## 8.1.3

### Patch Changes

- Updated dependencies [d774a3196]
  - @pnpm/types@9.3.0
  - @pnpm/lockfile-types@5.1.2
  - @pnpm/dependency-path@2.1.4
  - @pnpm/merge-lockfile-changes@5.0.4

## 8.1.2

### Patch Changes

- Updated dependencies [aa2ae8fe2]
  - @pnpm/types@9.2.0
  - @pnpm/lockfile-types@5.1.1
  - @pnpm/dependency-path@2.1.3
  - @pnpm/merge-lockfile-changes@5.0.3

## 8.1.1

### Patch Changes

- Updated dependencies [302ebffc5]
  - @pnpm/constants@7.1.1
  - @pnpm/error@5.0.2

## 8.1.0

### Minor Changes

- 9c4ae87bd: Some settings influence the structure of the lockfile, so we cannot reuse the lockfile if those settings change. As a result, we need to store such settings in the lockfile. This way we will know with which settings the lockfile has been created.

  A new field will now be present in the lockfile: `settings`. It will store the values of two settings: `autoInstallPeers` and `excludeLinksFromLockfile`. If someone tries to perform a `frozen-lockfile` installation and their active settings don't match the ones in the lockfile, then an error message will be thrown.

  The lockfile format version is bumped from v6.0 to v6.1.

  Related PR: [#6557](https://github.com/pnpm/pnpm/pull/6557)
  Related issue: [#6312](https://github.com/pnpm/pnpm/issues/6312)

### Patch Changes

- 9c4ae87bd: Convertion should work for all lockfile v6 formats, not just 6.0.
- Updated dependencies [9c4ae87bd]
- Updated dependencies [a9e0b7cbf]
- Updated dependencies [9c4ae87bd]
  - @pnpm/lockfile-types@5.1.0
  - @pnpm/types@9.1.0
  - @pnpm/constants@7.1.0
  - @pnpm/merge-lockfile-changes@5.0.2
  - @pnpm/dependency-path@2.1.2
  - @pnpm/error@5.0.1

## 8.0.2

### Patch Changes

- c0760128d: bump semver to 7.4.0
- Updated dependencies [c0760128d]
  - @pnpm/merge-lockfile-changes@5.0.1
  - @pnpm/dependency-path@2.1.1

## 8.0.1

### Patch Changes

- 5087636b6: Repeat installation should work on a project that has a dependency with () chars in the scope name [#6348](https://github.com/pnpm/pnpm/issues/6348).
- 94f94eed6: Installation should not fail when there is a local dependency that starts in a directory that starts with the `@` char [#6332](https://github.com/pnpm/pnpm/issues/6332).
- Updated dependencies [5087636b6]
- Updated dependencies [94f94eed6]
  - @pnpm/dependency-path@2.1.0

## 8.0.0

### Major Changes

- 158d8cf22: `useLockfileV6` field is deleted. Lockfile v5 cannot be written anymore, only transformed to the new format.
- eceaa8b8b: Node.js 14 support dropped.
- 417c8ac59: Create a lockfile even if the project has no dependencies at all.

### Patch Changes

- Updated dependencies [c92936158]
- Updated dependencies [ca8f51e60]
- Updated dependencies [eceaa8b8b]
- Updated dependencies [0e26acb0f]
  - @pnpm/lockfile-types@5.0.0
  - @pnpm/dependency-path@2.0.0
  - @pnpm/merge-lockfile-changes@5.0.0
  - @pnpm/constants@7.0.0
  - @pnpm/git-utils@1.0.0
  - @pnpm/error@5.0.0
  - @pnpm/types@9.0.0

## 7.0.6

### Patch Changes

- 787c43dcc: `patchedDependencies` are now sorted consistently in the lockfile [#6208](https://github.com/pnpm/pnpm/pull/6208).

## 7.0.5

### Patch Changes

- ed946c73e: Automatically fix conflicts in v6 lockfile.

## 7.0.4

### Patch Changes

- Updated dependencies [d89d7a078]
  - @pnpm/dependency-path@1.1.3

## 7.0.3

### Patch Changes

- Updated dependencies [9247f6781]
  - @pnpm/dependency-path@1.1.2

## 7.0.2

### Patch Changes

- 9a68ebbae: Fix lockfile v6.

## 7.0.1

### Patch Changes

- Updated dependencies [0f6e95872]
  - @pnpm/dependency-path@1.1.1

## 7.0.0

### Major Changes

- 3ebce5db7: Breaking change to the API of the read functions. Instead of one wanted lockfile version, it now expects an array of `wantedVersions`.

### Patch Changes

- Updated dependencies [3ebce5db7]
- Updated dependencies [3ebce5db7]
  - @pnpm/constants@6.2.0
  - @pnpm/dependency-path@1.1.0
  - @pnpm/error@4.0.1

## 6.0.5

### Patch Changes

- Updated dependencies [b77651d14]
  - @pnpm/types@8.10.0
  - @pnpm/lockfile-types@4.3.6
  - @pnpm/dependency-path@1.0.1
  - @pnpm/merge-lockfile-changes@4.0.3

## 6.0.4

### Patch Changes

- Updated dependencies [313702d76]
  - @pnpm/dependency-path@1.0.0

## 6.0.3

### Patch Changes

- a9d59d8bc: Update dependencies.

## 6.0.2

### Patch Changes

- Updated dependencies [702e847c1]
  - @pnpm/types@8.9.0
  - dependency-path@9.2.8
  - @pnpm/lockfile-types@4.3.5
  - @pnpm/merge-lockfile-changes@4.0.2

## 6.0.1

### Patch Changes

- Updated dependencies [844e82f3a]
  - @pnpm/types@8.8.0
  - dependency-path@9.2.7
  - @pnpm/lockfile-types@4.3.4
  - @pnpm/merge-lockfile-changes@4.0.1

## 6.0.0

### Major Changes

- f884689e0: Require `@pnpm/logger` v5.

### Patch Changes

- Updated dependencies [043d988fc]
- Updated dependencies [f884689e0]
  - @pnpm/error@4.0.0
  - @pnpm/merge-lockfile-changes@4.0.0

## 5.3.8

### Patch Changes

- 7c296fe9b: Update write-file-atomic to v4.

## 5.3.7

### Patch Changes

- Updated dependencies [e8a631bf0]
  - @pnpm/error@3.1.0

## 5.3.6

### Patch Changes

- Updated dependencies [d665f3ff7]
  - @pnpm/types@8.7.0
  - dependency-path@9.2.6
  - @pnpm/lockfile-types@4.3.3
  - @pnpm/merge-lockfile-changes@3.0.11

## 5.3.5

### Patch Changes

- Updated dependencies [156cc1ef6]
  - @pnpm/types@8.6.0
  - dependency-path@9.2.5
  - @pnpm/lockfile-types@4.3.2
  - @pnpm/merge-lockfile-changes@3.0.10

## 5.3.4

### Patch Changes

- 0373af22e: Always correctly update the "time" field in "pnpm-lock.yaml".

## 5.3.3

### Patch Changes

- 1e5482da4: Fix sorting of keys in lockfile to make it more deterministic and prevent unnecessary churn in the lockfile [#5151](https://github.com/pnpm/pnpm/pull/5151).

## 5.3.2

### Patch Changes

- 8103f92bd: Use a patched version of ramda to fix deprecation warnings on Node.js 16. Related issue: https://github.com/ramda/ramda/pull/3270
- Updated dependencies [8103f92bd]
  - @pnpm/merge-lockfile-changes@3.0.9

## 5.3.1

### Patch Changes

- 44544b493: Don't incorrectly identify a lockfile out-of-date when the package has a publishConfig.directory field [#5124](https://github.com/pnpm/pnpm/issues/5124).
- Updated dependencies [c90798461]
  - @pnpm/types@8.5.0
  - @pnpm/lockfile-types@4.3.1
  - @pnpm/merge-lockfile-changes@3.0.8

## 5.3.0

### Minor Changes

- 8dcfbe357: Add `publishDirectory` field to the lockfile and relink the project when it changes.

### Patch Changes

- Updated dependencies [8dcfbe357]
  - @pnpm/lockfile-types@4.3.0
  - @pnpm/merge-lockfile-changes@3.0.7

## 5.2.0

### Minor Changes

- 4fa1091c8: Add experimental lockfile format that should merge conflict less in the `importers` section. Enabled by setting the `use-inline-specifiers-lockfile-format = true` feature flag in `.npmrc`.

  If this feature flag is committed to a repo, we recommend setting the minimum allowed version of pnpm to this release in the `package.json` `engines` field. Once this is set, older pnpm versions will throw on invalid lockfile versions.

## 5.1.4

### Patch Changes

- ab684d77e: Never add an empty patchedDependencies field to `pnpm-lock.yaml`.

## 5.1.3

### Patch Changes

- 5f643f23b: Update ramda to v0.28.
- Updated dependencies [5f643f23b]
  - @pnpm/merge-lockfile-changes@3.0.6

## 5.1.2

### Patch Changes

- Updated dependencies [d01c32355]
- Updated dependencies [8e5b77ef6]
- Updated dependencies [8e5b77ef6]
  - @pnpm/lockfile-types@4.2.0
  - @pnpm/types@8.4.0
  - @pnpm/merge-lockfile-changes@3.0.5

## 5.1.1

### Patch Changes

- Updated dependencies [2a34b21ce]
  - @pnpm/types@8.3.0
  - @pnpm/lockfile-types@4.1.0
  - @pnpm/merge-lockfile-changes@3.0.4

## 5.1.0

### Minor Changes

- 56cf04cb3: New settings added: use-git-branch-lockfile, merge-git-branch-lockfiles, merge-git-branch-lockfiles-branch-pattern.

### Patch Changes

- Updated dependencies [fb5bbfd7a]
- Updated dependencies [56cf04cb3]
  - @pnpm/types@8.2.0
  - @pnpm/git-utils@0.1.0
  - @pnpm/lockfile-types@4.0.3
  - @pnpm/merge-lockfile-changes@3.0.3

## 5.0.4

### Patch Changes

- Updated dependencies [4d39e4a0c]
  - @pnpm/types@8.1.0
  - @pnpm/lockfile-types@4.0.2
  - @pnpm/merge-lockfile-changes@3.0.2

## 5.0.3

### Patch Changes

- 52b0576af: feat: support libc filed

## 5.0.2

### Patch Changes

- Updated dependencies [18ba5e2c0]
  - @pnpm/types@8.0.1
  - @pnpm/lockfile-types@4.0.1
  - @pnpm/merge-lockfile-changes@3.0.1

## 5.0.1

### Patch Changes

- Updated dependencies [1267e4eff]
  - @pnpm/constants@6.1.0
  - @pnpm/error@3.0.1

## 5.0.0

### Major Changes

- 542014839: Node.js 12 is not supported.

### Patch Changes

- Updated dependencies [d504dc380]
- Updated dependencies [542014839]
  - @pnpm/types@8.0.0
  - @pnpm/constants@6.0.0
  - @pnpm/error@3.0.0
  - @pnpm/lockfile-types@4.0.0
  - @pnpm/merge-lockfile-changes@3.0.0

## 4.3.1

### Patch Changes

- Updated dependencies [70ba51da9]
  - @pnpm/error@2.1.0

## 4.3.0

### Minor Changes

- b138d048c: New optional field supported: `onlyBuiltDependencies`.

### Patch Changes

- Updated dependencies [b138d048c]
  - @pnpm/lockfile-types@3.2.0
  - @pnpm/types@7.10.0
  - @pnpm/merge-lockfile-changes@2.0.8

## 4.2.6

### Patch Changes

- Updated dependencies [26cd01b88]
  - @pnpm/types@7.9.0
  - @pnpm/lockfile-types@3.1.5
  - @pnpm/merge-lockfile-changes@2.0.7

## 4.2.5

### Patch Changes

- Updated dependencies [b5734a4a7]
  - @pnpm/types@7.8.0
  - @pnpm/lockfile-types@3.1.4
  - @pnpm/merge-lockfile-changes@2.0.6

## 4.2.4

### Patch Changes

- eb9ebd0f3: In a dedicated lockfile the `dependenciesMeta` field should be nested to `'.'` during normalization.
- eb9ebd0f3: The `dependenciesMeta` field should be sorted after the dependencies fields.

## 4.2.3

### Patch Changes

- Updated dependencies [6493e0c93]
  - @pnpm/types@7.7.1
  - @pnpm/lockfile-types@3.1.3
  - @pnpm/merge-lockfile-changes@2.0.5

## 4.2.2

### Patch Changes

- Updated dependencies [ba9b2eba1]
  - @pnpm/types@7.7.0
  - @pnpm/lockfile-types@3.1.2
  - @pnpm/merge-lockfile-changes@2.0.4

## 4.2.1

### Patch Changes

- Updated dependencies [302ae4f6f]
  - @pnpm/types@7.6.0
  - @pnpm/lockfile-types@3.1.1
  - @pnpm/merge-lockfile-changes@2.0.3

## 4.2.0

### Minor Changes

- 4ab87844a: New optional property added to project snapshots: `dependenciesMeta`.

### Patch Changes

- Updated dependencies [4ab87844a]
- Updated dependencies [4ab87844a]
  - @pnpm/types@7.5.0
  - @pnpm/lockfile-types@3.1.0
  - @pnpm/merge-lockfile-changes@2.0.2

## 4.1.1

### Patch Changes

- Updated dependencies [b734b45ea]
  - @pnpm/types@7.4.0

## 4.1.0

### Minor Changes

- 8e76690f4: New optional field added to the lockfile: `packageExtensionsChecksum`.

### Patch Changes

- Updated dependencies [8e76690f4]
  - @pnpm/types@7.3.0

## 4.0.4

### Patch Changes

- 2dc5a7a4c: Values of properties in the engines field should be written to single line.

## 4.0.3

### Patch Changes

- Updated dependencies [724c5abd8]
  - @pnpm/types@7.2.0

## 4.0.2

### Patch Changes

- a1a03d145: Import only the required functions from ramda.
- Updated dependencies [a1a03d145]
  - @pnpm/merge-lockfile-changes@2.0.1

## 4.0.1

### Patch Changes

- Updated dependencies [97c64bae4]
  - @pnpm/types@7.1.0

## 4.0.0

### Major Changes

- 97b986fbc: Node.js 10 support is dropped. At least Node.js 12.17 is required for the package to work.

### Minor Changes

- 155e70597: The "resolution" field should always be the first key. This will reduce the number of issues during lockfile merges.
- f7750baed: Add blank lines to the lockfile between items.

  The `resolution` object should be written in a single line.

### Patch Changes

- 9c2a878c3: Change order of keys in package snapshot.
- 8b66f26dc: Do not fail when `lockfileVersion` is a string.
- 9c2a878c3: Write engines, os, and cpu to single line.
- Updated dependencies [6871d74b2]
- Updated dependencies [97b986fbc]
- Updated dependencies [6871d74b2]
- Updated dependencies [f2bb5cbeb]
  - @pnpm/constants@5.0.0
  - @pnpm/error@2.0.0
  - @pnpm/lockfile-types@3.0.0
  - @pnpm/merge-lockfile-changes@2.0.0
  - @pnpm/types@7.0.0

## 3.2.1

### Patch Changes

- 51e1456dd: Throw a standard pnpm error object on broken lockfile error. The error code is `ERR_PNPM_BROKEN_LOCKFILE`.

## 3.2.0

### Minor Changes

- 9ad8c27bf: Add optional neverBuiltDependencies property to the lockfile object.

### Patch Changes

- Updated dependencies [9ad8c27bf]
- Updated dependencies [9ad8c27bf]
  - @pnpm/lockfile-types@2.2.0
  - @pnpm/types@6.4.0
  - @pnpm/merge-lockfile-changes@1.0.1

## 3.1.4

### Patch Changes

- af897c324: An empty overrides field should be removed from the lockfile before saving.

## 3.1.3

### Patch Changes

- 1e4a3a17a: Update js-yaml to version 4.

## 3.1.2

### Patch Changes

- fba715512: writeLockfiles should return Promise<void>.

## 3.1.1

### Patch Changes

- Updated dependencies [0c5f1bcc9]
  - @pnpm/error@1.4.0

## 3.1.0

### Minor Changes

- 3776b5a52: New function added that reads the lockfile and autofixes any merge conflicts.

### Patch Changes

- Updated dependencies [3776b5a52]
  - @pnpm/merge-lockfile-changes@1.0.0

## 3.0.18

### Patch Changes

- dbcc6c96f: Print a better error message when stringifying a lockfile object fails.
- 09492b7b4: Update write-file-atomic to v3.

## 3.0.17

### Patch Changes

- aa6bc4f95: Print a better when stringifying a lockfile object fails.

## 3.0.16

### Patch Changes

- Updated dependencies [b5d694e7f]
  - @pnpm/lockfile-types@2.1.1
  - @pnpm/types@6.3.1

## 3.0.15

### Patch Changes

- Updated dependencies [d54043ee4]
- Updated dependencies [d54043ee4]
- Updated dependencies [fcdad632f]
  - @pnpm/lockfile-types@2.1.0
  - @pnpm/types@6.3.0
  - @pnpm/constants@4.1.0

## 3.0.14

### Patch Changes

- Updated dependencies [75a36deba]
  - @pnpm/error@1.3.1

## 3.0.13

### Patch Changes

- 9550b0505: Remove the `packages` field before saving, if it equals `undefined`.

## 3.0.12

### Patch Changes

- Updated dependencies [6d480dd7a]
  - @pnpm/error@1.3.0

## 3.0.11

### Patch Changes

- Updated dependencies [db17f6f7b]
  - @pnpm/types@6.2.0

## 3.0.10

### Patch Changes

- Updated dependencies [71a8c8ce3]
  - @pnpm/types@6.1.0

## 3.0.9

### Patch Changes

- Updated dependencies [b5f66c0f2]
- Updated dependencies [ca9f50844]
- Updated dependencies [da091c711]
- Updated dependencies [6a8a97eee]
- Updated dependencies [4f5801b1c]
  - @pnpm/constants@4.0.0
  - @pnpm/types@6.0.0
  - @pnpm/lockfile-types@2.0.1
  - @pnpm/error@1.2.1

## 3.0.9-alpha.2

### Patch Changes

- Updated dependencies [ca9f50844]
- Updated dependencies [6a8a97eee]
  - @pnpm/constants@4.0.0-alpha.1
  - @pnpm/lockfile-types@2.0.1-alpha.0

## 3.0.9-alpha.1

### Patch Changes

- Updated dependencies [da091c71]
  - @pnpm/types@6.0.0-alpha.0

## 3.0.9-alpha.0

### Patch Changes

- Updated dependencies [b5f66c0f2]
  - @pnpm/constants@4.0.0-alpha.0

## 3.0.8

### Patch Changes

- 907c63a48: Dependencies updated.
- 907c63a48: Dependencies updated.
- 907c63a48: Use `fs.mkdir` instead of `make-dir`.
