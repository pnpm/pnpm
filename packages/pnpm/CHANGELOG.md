# pnpm

## 5.13.4

### Patch Changes

- Issue with Homebrew fixed [#2993]https://github.com/pnpm/pnpm/issues/2993).

## 5.13.3

- Fix regression with node-gyp that was introduced in v5.13.2 [#2988](https://github.com/pnpm/pnpm/issues/2988).

## 5.13.2

### Patch Changes

- The pnpm CLI is bundled for faster startup.

## 5.13.1

### Patch Changes

- pnpm should not leave empty temporary directories in the root of the partition [#2749](https://github.com/pnpm/pnpm/issues/2749).

## 5.13.0

### Minor Changes

- New setting added: `prefer-workspace-packages` [#2136](https://github.com/pnpm/pnpm/issues/2136).

  When `prefer-workspace-packages` is set to `true`, local packages from the workspace are preferred over
  packages from the registry, even if there is a newer version of that package in the registry.

  This setting is only useful if the workspace doesn't use `save-workspace-protocol=true`.

## 5.11.2

### Minor Changes

- Workspace packages now can be referenced through aliases [#2970](https://github.com/pnpm/pnpm/issues/2970).

  For instance, the package in the workspace may be named `foo`. Usually, you would reference it as `{ "foo": "workspace:*" }`.
  If you want to use a different alias, the next syntax will work now: `{ "bar": "workspace:foo@*" }`.

  Before publish, aliases are converted to regular aliased dependencies. The above example will become: `{ "bar": "npm:foo@1.0.0" }`.

- Workspace packages now can be referenced through relative path [#2959](https://github.com/pnpm/pnpm/issues/2959).

  For example, in a workspace with 2 packages:

  ```
  + packages
    + foo
    + bar
  ```

  `bar` may have `foo` in its dependencies declared as `{ "foo": "workspace:../foo" }`. Before publish, these specs are converted to regular version specs supported by all package managers.

- For better compatibility with prettier, two new default patterns added to `public-hoist-pattern`:
  - `@prettier/plugin-*`
  - `*prettier-plugin-*`

## 5.11.1

### Patch Changes

- Retry metadata download if the received JSON is broken [#2949](https://github.com/pnpm/pnpm/issues/2949).

## 5.11.0

### Minor Changes

- Conflicts in `pnpm-lock.yaml` are automatically fixed by `pnpm install` [#2965](https://github.com/pnpm/pnpm/pull/2965).

## 5.10.4

### Patch Changes

- Don't ignore the `overrides` field of the root project, when the scope of the command doesn't include the root project.

## 5.10.2

### Patch Changes

- When extracting packages to the store, file duplicates are skipped.
- When creating a hard link fails, fall back to copying only if the target file does not exist.

## 5.10.1

### Minor Changes

- A `"pnpm"."overrides"` field may be used to override version ranges of dependencies.
  The overrides field can be specified only in the root project's `package.json`.

  An example of the `"pnpm"."overrides"` field:

  ```json
  {
    "pnpm": {
      "overrides": {
        "foo": "^1.0.0",
        "bar@^2.1.0": "3.0.0",
        "qar@1>zoo": "2"
      }
    }
  }
  ```

  You may specify the package to which the overriden dependency belongs by separating the package selector from the dependency selector with a ">", for example `qar@1>zoo` will only override the `zoo` dependency of any `qar@1` dependency.

- A new setting added for specifying the shell to use, when running scripts: script-shell [#2942](https://github.com/pnpm/pnpm/issues/2942)

- When some of the dependencies of a package have the package as a peer depenendency, don't make the dependency a peer depenendency of itself.

- Lockfile version bumped to 5.2

## 5.9.3

### Patch Changes

- Fixes a regression with CLI commands inside a workspace introduced in v5.9.0 [#2925](https://github.com/pnpm/pnpm/issues/2925)

## 5.9.2

### Patch Changes

- Fixed multiple issues with inconsistent lockfile generation [#2919](https://github.com/pnpm/pnpm/issues/2919)

## 5.9.0

### Minor Changes

- Plug'n'Play support added [#2902](https://github.com/pnpm/pnpm/issues/2902)

  To use Plug'n'Play in a project, create a `.npmrc` file in its root with the following content:

  ```ini
  node-linker=pnp

  ; Setting symlink to false is optional.
  symlink=false
  ```

  All the commands will work, when executed through `pnpm run`.
  However, directly executing a `.js` file with Node.js will fail. Node's
  resolver should be patched with `.pnp.js`. So instead of `node index.js`, you should
  run: `node --require=./.pnp.js index.js`

- New setting: `symlink` [#2900](https://github.com/pnpm/pnpm/pull/2900)

  When `symlink` is set to `false`, pnpm creates a virtual store directory (`node_modules/.pnpm`) without any symlinks.

### Patch Changes

- Fixed some edge cases with resolving peer dependencies [#2919](https://github.com/pnpm/pnpm/issues/2919).
- Installation should fail if there are references to a project that has been removed from the workspace [#2905](https://github.com/pnpm/pnpm/pull/2905).
- pnpm should not suggest to update pnpm to a newer version, when the installed version is bigger than latest.

## 5.8.0

### Minor Changes

- New setting: `shell-emulator` [#2621](https://github.com/pnpm/pnpm/issues/2621)

  When `shell-emulator` is `true`, pnpm will use a shell emulator to execute scripts. So things like `FOO=1 pnpm run foo` and other simple bash syntax will work on Windows.

  pnpm uses the shell emulator that was developed for Yarn v2: [@yarnpkg/shell](https://www.npmjs.com/package/@yarnpkg/shell).

- Exluding projects using `--filter=!<selector>` [#2804](https://github.com/pnpm/pnpm/issues/2804)

  Packages may be excluded from a command's scope, using "!" at the beginning of the selector.

  For instance, this will run tests in all projects except `foo`:

  ```
  pnpm --filter=!foo test
  ```

  And this one will run tests in all projects that are not under the `lib` directory:

  ```
  pnpm --filter=!./lib test
  ```

### Patch Changes

- When searching for a global bin directory, also look for symlinked commands [#2888](https://github.com/pnpm/pnpm/issues/2888).
- Don’t remove non‑pnpm `.dot_files` from `node_modules` [#2833](https://github.com/pnpm/pnpm/pull/2833).
- During publish, check the active branch name after checking if the branch is clean.
- The `INIT_CWD` env variable is always set to the lockfile directory for scripts of dependencies [#2897](https://github.com/pnpm/pnpm/pull/2897).
- When a package is both a dev dependency and a prod dependency, the package should be linked when installing prod dependencies only. This was an issue only when a lockfile was not present during installation [#2882](https://github.com/pnpm/pnpm/issues/2882).

## 5.7.0

### Minor Changes

- Performance improvements:
  - If a file in the store was never modified, we are not checking its integrity ([#2876](https://github.com/pnpm/pnpm/pull/2876)).
  - All directories in the virtual store are created before symlinking and importing packages starts ([#2875](https://github.com/pnpm/pnpm/pull/2875)).

## 5.6.1

### Patch Changes

- Fixing a regression introduced in v5.5.13. Installation should never fail during automatic importing method selection (#2869).

## 5.6.0

### Minor Changes

- `--workspace-root`, `-w`: a new option that allows to focus on the root workspace project.

  E.g., the following command runs the `lint` script of the root `package.json` from anywhere in the monorepo:

  ```
  pnpm -w lint
  ```

  PR #2866

- The progress indicator also shows the number of dependencies that are being added to the modules direcotory (#2832).

- Don't report scope, when only one workspace package is selected (#2855).

- If a script is not found in the current project but is present in the root project of the workspace, notify the user about it in the hint of the error (#2859).

- Publicly hoist anything that has "types" in the name. Packages like `@babel/types` are publicly hoisted by default (#2865).

### Patch Changes

- When no matching version is found, report the actually specified version spec in the error message (not the normalized one) (#1314).

## 5.5.13

### Patch Changes

- When `package-import-method` is set to `auto`, cloning is only tried once. If it fails, it is not retried for other packages.
- More information added to the Git check errors and prompt.
- Report an info log instead of a warning when some binaries cannot be linked.

## 5.5.12

### Patch Changes

- In some rare cases, `pnpm install --no-prefer-frozen-lockfile` didn't link the direct dependencies to the root `node_modules`. This was happening when the direct dependency was also resolving some peer dependencies.

## 5.5.11

### Patch Changes

- Sometimes, when installing new dependencies that rely on many peer dependencies, or when running installation on a huge monorepo, there will be hundreds or thousands of warnings. Printing many messages to the terminal is expensive and reduces speed, so pnpm will only print a few warnings and report the total number of the unprinted warnings.
- `pnpm outdated --long` should print details about the outdated commands.

## 5.5.10

### Patch Changes

- Fixing a regression that was shipped with pnpm v5.5.6. Cyclic dependencies that have peer dependencies were not symlinked to the root of `node_modules`, when they were direct dependencies.

## 5.5.9

### Patch Changes

- Always try to resolve optional peer dependencies. Fixes a regression introduced in pnpm v5.5.8

## 5.5.8

### Patch Changes

- "Heap out of memory" error fixed, which happened on some huge projects with big amount of peer dependencies, since pnpm v3.4.0 (#2339).

## 5.5.7

### Patch Changes

- Ignore non-array bundle\[d]Dependencies fields. Fixes a regression caused by https://github.com/pnpm/pnpm/commit/5322cf9b39f637536aa4775aa64dd4e9a4156d8a

## 5.5.6

### Patch Changes

- "Heap out of memory" error fixed, which happened on some huge projects with big amount of peer dependencies, since pnpm v3.4.0 (#2339).
- `pnpm add --global <pkg>` should not break the global package, when the `save` setting is set to `false` (#2261).
- `pnpm test|start|stop` should allow the same options as `pnpm run test|start|stop` (#2814).
- Improve the error message on 404 errors. Include authorization details (#2818).

## 5.5.5

### Patch Changes

- Generate a valid lockfile, when the same dependency is specified both in `devDependencies` and `optionalDependencies` (#2807).
- It should be possible to set the fetch related options through CLI options (#2810).
- Fix a regression introduced to `pnpm run --parallel <script>` in pnpm v5.5.4.

## 5.5.4

### Patch Changes

- Any ESLint related dependencies are publicly hoisted by default (#2799).
- `pnpm install -r` should recreate the modules directory
  if the hoisting patterns were updated in a local config file.
  The hoisting patterns are configured via the `hoist-pattern`
  and `public-hoist-pattern` settings (#2802).
- The same code should run when running some command inside a project directory,
  or when using `--filter` to select a specific workspace project (#2805).

  This fixes an issue that was happening when running `pnpm add pkg` inside a workspace.
  The issue was not reproducible when running `pnpm add pkg --filter project` (#2798).

## 5.5.3

### Patch Changes

- pnpm should not always suggest to reinstall the modules directory, when `public-hoist-pattern` is set to nothing (#2783).
- When searching for a suitable directory for the global executables, search for node, npm, pnpm files only, not directories (#2793).

## 5.5.2

### Patch Changes

- `pnpm publish -r` does not publish packages with the temporary `pnpm-temp` distribution tag (#2686).
- Print the authorization settings (with hidden private info), when an authorization error happens during fetch (#2774).

## 5.5.1

### Patch Changes

- Stop looking for project root not only when `package.json` or `node_modules` is found
  but also on `package.json5` and `package.yaml`.

## 5.5.0

### Minor Changes

- Allow unknown options that are prefixed with `config.`

  `pnpm install --foo` would fail with an unknown option error.

  `pnpm install --config.foo` will work fine, setting the `npm_config_foo` environment variable for child lifecycle events.

### Patch Changes

- Don't leave empty temp directories in home directory (#2749).
- Reunpack the contents of a modified tarball dependency (#2747).
- `pnpm list -r` should print the legend only once.
- Don't read the `.npmrc` file that is outside the workspace.
- Hoisting should work in a workspace that has no root project.

## 5.4.12

### Patch Changes

- Fixing regression of v5.4.5: the `pnpm update` command should update the direct dependencies of the project.

## 5.4.11

### Patch Changes

- Fixing regression of v5.4.10: a warning during `package.json` save.

## 5.4.10

### Patch Changes

- Don't add a trailing new line to `package.json` if no trailing new line was present in it (#2716).
- Installing a new dependency with a trailing `@` (#2737).
- Ignore files in the modules directory (#2730).

## 5.4.9

### Patch Changes

- Get the right package name of a package resolved from GitHub registry (#2734).
- Registry set in lockfile resolution should not be ignored (#2733).
- Workspace range prefix should be removed from `peerDependencies` before publish (#2467).
- Use the same versions of dependencies across the pnpm monorepo.
- Fix lockfile not updated when remove dependency in project with readPackage hook (#2726).

## 5.4.8

### Patch Changes

- `pnpm audit --audit-level high` should not error if the found vulnerabilities are low and/or moderate (#2721).
- When purging an incompatible modules directory, don't remove the actual directory, just the contents of it (#2720).

## 5.4.7

### Patch Changes

- `pnpm outdated` should exit with exit code 1, when there are outdated dependencies.
- `pnpm audit` should exit with exit code 1, when vulnerabilities are found.
- `pnpm install --prod --frozen-lockfile` should not fail if there are dev dependencies used as peer dependencies of prod dependencies (#2711).

## 5.4.6

### Patch Changes

- `pnpm root -g` should not fail if pnpm has no write access to the global bin directory (#2700).

## 5.4.5

### Patch Changes

- `pnpm update dep --depth Infinity` should only update `dep`.
- `pnpm publish -r --dry-run` should not publish anything to the registry.

## 5.4.4

### Patch Changes

- `pnpm root -g` should not fail if pnpm has no write access to the global bin directory (#2700).
- Suggest to use pnpm to update pnpm.

## 5.4.3

### Patch Changes

- Should not print colored output when `color` is set to `never`. This was an issue in commands that don't use `@pnpm/default-reporter`. Commands like `pnpm list`, `pnpm outdated`.
- Changes that are made by the `readPackage` hook are not saved to the `package.json` files of projects.
- Allow the `--registry` option with the `pnpm audit` command.
- Allow the `--save-workspace-protocol` option.
- Don't use inversed colors to highlight search results in `pnpm list`, `pnpm why`.

## 5.4.2

### Patch Changes

- `pnpm install` should work as `pnpm install --filter {.}...`, when `recursive-install` is `false`.
- On first install, print an info message about the package importing (hard linking, cloning, or copying) method
  and the location of the virtual store and the content-addressable store.

## 5.4.0

### Minor Changes

- Installation of private Git-hosted repositories via HTTPS using an auth token.

  ```text
  pnpm add git+https://{token}:x-oauth-basic@github.com/SOME_ORG/SOME_PRIVATE_REPO.git
  ```

- A new setting called `recursive-install` was added. When it is set to `false`, `pnpm install` will only install dependencies in current project, even when executed inside a monorepo.

  If `recursive-install` is `false`, you should explicitly run `pnpm install -r` in order to install all dependencies in all workspace projects.

- Projects that don't have a `"version"` field may be installed as dependencies of other projects in the workspace, using the `"workspace:0.0.0"` specifier.

  So if there's `foo` in the repository that has no version field, `bar` may have it as a dependency:

  ```json
  "dependencies": {
    "foo": "workspace:0.0.0"
  }
  ```

- By default, all ESLint plugin are hoisted to the root of `node_modules`.

  `eslint-plugin-*` added as one of the default patterns of `public-hoist-pattern`.

- Improved error message on workspace range resolution error.

  Now the path to the project is printed, where the error originated.

### Patch Changes

- `pnpm prune` should accept the `--no-optional` and `--no-dev` options.

## 5.3.0

### Minor Changes

- Any unknown command is assumed to be a script. So `pnpm foo` becomes `pnpm run foo`.

### Patch Changes

- Fix installation of packages via repository URL. E.g., `pnpm add https://github.com/foo/bar`.

## 5.2.9

### Patch Changes

- `run --silent <cmd>` should only print output of the command and nothing from pnpm (#2660).
- installing a new optional dependency that has an option dependency should not fail (#2663).

## 5.2.8

### Patch Changes

- Fixing some issues with finding the target directory for command files during global installation.

## 5.2.7

### Patch Changes

- Fixing some issues with finding the target directory for command files during global installation.

## 5.2.6

### Patch Changes

- Use the proxy settings not only during resolution but also when fetching tarballs.

## 5.2.5

### Patch Changes

- Fixing some regressions with global bin directory caused by v5.2.4.

## 5.2.4

### Patch Changes

- Find the proper directory for linking executables during global installation.
- Fix `pnpm list --long`. This was a regression in pnpm v5.0.0.

## 5.2.3

### Patch Changes

- Own implementation of the `pnpm bin` command added (previously it was passed through to `npm bin`).
- Read the correct PATH env variable on all systems, when running pnpx. One Windows the correct path name might be Path or other.
- Install the pnpm bin executable to the directory of the globally installed pnpm executable, when running `pnpm add -g pnpm`.
- `pnpm store prune` should not fail when the store has some foreign files.
- `pnpm unlink --global` should unlink bin files from the global executables directory.

## 5.2.2

### Patch Changes

- Don't remove skipped optional dependencies from the current lockfile on partial installation.

## 5.2.1

### Patch Changes

- Hoisting should not fail if some of the aliases cannot be hoisted due to issues with the lockfile.

## 5.2.0

### Minor Changes

- Added a new setting: `public-hoist-pattern`. This setting can be overwritten by `shamefully-hoist`. The default value of `public-hoist-pattern` is `types/*`.

  If `shamefully-hoist` is `true`, `public-hoist-pattern` is set to `*`.

  If `shamefully-hoist` is `false`, `public-hoist-pattern` is set to nothing.

  `public-hoist-pattern` example configuration (through a `.npmrc` file):

  ```
  public-hoist-pattern[]=@types/*
  public-hoist-pattern[]=@angular/*
  ```

  Related PR: #2631

- Don't request the full metadata of package when running `pnpm outdated` or `pnpm publish -r` (#2633)

## 5.1.8

### Patch Changes

- Don't fail when the installed package's manifest (`package.json`) starts with a byte order mark (BOM). This is a fix for a regression that appeared in v5.0.0 (#2629).

## 5.1.7

### Patch Changes

- When `link-workspace-packages` is `false`, filtering by dependencies/dependents should ignore any packages that are not specified via `workspace:` ranges (#2625).
- Print a "Did you mean" line under the unknown option error with any option that look similar to the typed one (#2603).

## 5.1.6

### Patch Changes

- It should be possible to install a tarball through a non-standard URL endpoint served via the registry domain.

  For instance, the configured registry is `https://registry.npm.taobao.org/`.
  It should be possible to run `pnpm add https://registry.npm.taobao.org/vue/download/vue-2.0.0.tgz`

  Related issue: #2549

## 5.1.5

### Patch Changes

- Print a warning when an HTTP request fails (#2615).
- Perform headless installation when dependencies should not be linked from the workspace, and they are not indeed linked from the workspace (#2619).

## 5.1.4

### Patch Changes

- Fix too long file name issue during write to the content-addressable store (#2605).

## 5.1.3

### Patch Changes

- Don't remove authorization headers when redirecting requests to the same host (#2602).

## 5.1.2

### Patch Changes

- fix an issue with node-gyp failure. Downgrade uuid.

## 5.1.1

### Patch Changes

- 86d21759d: Print a meaningful error when pnpm is executed with Node.js v13.0-v13.6

## 5.1.0

### Minor Changes

- ffddf34a8: Add new global option called `--stream`. (#2595)

  When used, the output from child processes is streamed to the console immediately, prefixed with the originating package directory. This allows output from different packages to be interleaved.

- The `run` and `exec` commands may use the `--parallel` option.

  `--parallel` completely disregards concurrency and topological sorting,
  running a given script immediately in all matching packages
  with prefixed streaming output. This is the preferred flag
  for long-running processes such as watch run over many packages.

  For example: `pnpm run --parallel watch`

  PR #2599

- Color the child output prefixes (#2598)

### Patch Changes

- A recursive run should not rerun the same package script which started the lifecycle event (#2528).
- Fixing a regression on Windows. Fall back to copying if linking fails (429c5a560b7a32b0261e471ece349ec136ab7f4d)

## 5.0.2

### Patch Changes

- 2f9c7ca85: Fix a regression introduced in pnpm v5.0.0.
  Create correct lockfile when the package tarball is hosted not under the registry domain.
- 160975d62: This fixes a regression introduced in pnpm v5.0.0. Direct local tarball dependencies should always be reanalized on install.

## 5.0.1

### Patch Changes

- 81b537003: The usage of deprecated options should not crash the CLI. When a deprecated option is used (like `pnpm install --no-lock`), just print a warning.
- 187615f87: Fix installation of git-hosted packages. This was a regression in v5.

## 5.0.0

### Major Changes

- 🚀 33% faster installation times vs pnpm v4.

  In some cases, 2 times faster than Yarn v1! ([performance diff of pnpm v4 vs v5](https://github.com/pnpm/benchmarks-of-javascript-package-managers/commit/5328f0165628b0ee5e22a8a433357d65bee75d64))

  | action  | cache | lockfile | node_modules | npm   | pnpm  | Yarn  | Yarn PnP |
  | ------- | ----- | -------- | ------------ | ----- | ----- | ----- | -------- |
  | install |       |          |              | 43.3s | 17.5s | 36.7s | 28.6s    |
  | install | ✔     | ✔        | ✔            | 7s    | 1.5s  | 735ms | n/a      |
  | install | ✔     | ✔        |              | 18.3s | 7.8s  | 10.5s | 1.8s     |
  | install | ✔     |          |              | 24.8s | 10.9s | 22.2s | 12.1s    |
  | install |       | ✔        |              | 23.2s | 15.2s | 22.4s | 13.4s    |
  | install | ✔     |          | ✔            | 6.4s  | 1.8s  | 17.1s | n/a      |
  | install |       | ✔        | ✔            | 7.3s  | 1.5s  | 735ms | n/a      |
  | install |       |          | ✔            | 6.4s  | 3.1s  | 33.2s | n/a      |
  | update  | n/a   | n/a      | n/a          | 7s    | 14.5s | 42.6s | 27.6s    |

  All the benchmarks are [here](https://github.com/pnpm/benchmarks-of-javascript-package-managers/tree/5328f0165628b0ee5e22a8a433357d65bee75d64).

- A content-addressable filesystem is used to store packages on the disk.

  pnpm v5 uses a content-addressable filesystem to store all files from all module directories on a disk. If you depend on different versions of lodash, only the files that differ are added to the store. If lodash has 100 files, and a new version has a change only in one of those files, pnpm update will only add 1 new file to the storage.

  For more info about the structure of this new store, you can check the [GitHub issue about it](https://github.com/pnpm/pnpm/issues/2470).

  This change was inspired by [dupe-krill](https://github.com/kornelski/dupe-krill) and the content-addressable storage of Git.

- Reduced directory nesting in the virtual store directory.

  In pnpm v4, if you installed `foo@1.0.0`, it was hard-linked into `node_modules/.pnpm/registry.npmjs.org/foo/1.0.0/`.

  In pnpm v5, it will be hard-linked into `node_modules/.pnpm/foo@1.0.0/`. This new structure of the virtual store directory drastically reduces the number of directories pnpm has to create. Hence, there are fewer filesystem operations, which improves speed.

- `pnpm store usages` removed.

  This command was using information from the `store.json` files, which is not present in the new content-addressable storage anymore.

- The `independent-leaves` setting has been removed.

  When hoisting was off, it was possible to set the `independent-leaves` setting to `true`. When `true`, leaf dependencies were symlinked directly from the global store. However, we turned hoisting on by default for pnpm v4, so this feature has no future at the moment.

- The `resolution-strategy` setting has been removed.

  By default, the `fewer-dependencies` resolution strategy is used. It was possible to select a `fast` resolution strategy. This setting is deprecated to simplify future improvements to the resolution algorithm.

- The store and the modules directory are not locked.

  We are not using directory locks anymore. So the `--no-lock` option will throw an error. Some users had [issues](https://github.com/pnpm/pnpm/issues/594) with locking. We have confidence that pnpm will never leave either node_modules or the store in a broken state,
  so we removed locking.

- `git-checks` is `true` by default.

  By default, `pnpm publish` will make some checks before actually publishing a new version of your package.

  The next checks will happen:

  - The current branch is your publish branch. The publish branch is `master` by default. This is configurable through the `publish-branch` setting.
  - Your working directory is clean (there are no uncommitted changes).
  - The branch is up-to-date.

  If you don't want this checks, run `pnpm publish --no-git-checks` or set this setting to `false` via a `.npmrc` file.

- In case of a crash, the debug file will be written to `node_modules/.pnpm-debug.log` (not to `pnpm-debug.log` as in v4 and earlier).

### Minor Changes

- The `link-workspace-packages` setting may now be set to `deep`.

  When `link-workspace-packages` is set to `deep`, packages from the workspace will be linked even to subdependencies.
