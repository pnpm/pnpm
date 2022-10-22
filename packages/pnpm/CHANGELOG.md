# pnpm

## 7.14.0

### Minor Changes

- Add `pnpm doctor` command to do checks for known common issues

### Patch Changes

- Ignore the `always-auth` setting.

  pnpm will never reuse the registry auth token for requesting the package tarball, if the package tarball is hosted on a different domain.

  So, for example, if your registry is at `https://company.registry.com/` but the tarballs are hosted at `https://tarballs.com/`, then you will have to configure the auth token for both domains in your `.npmrc`:

  ```
  @my-company:registry=https://company.registry.com/
  //company.registry.com/=SOME_AUTH_TOKEN
  //tarballs.com/=SOME_AUTH_TOKEN
  ```

## 7.13.6

### Patch Changes

- Downgrade `@pnpm/npm-conf` to remove annoying builtin warning [#5518](https://github.com/pnpm/pnpm/issues/5518).
- `pnpm link --global <pkg>` should not change the type of the dependency [#5478](https://github.com/pnpm/pnpm/issues/5478).
- When the `pnpm outdated` command fails, print in which directory it failed.

## 7.13.5

### Patch Changes

- Print a warning when cannot read the builtin npm configuration.
- Also include missing deeply linked workspace packages at headless installation [#5034](https://github.com/pnpm/pnpm/issues/5034).
- `pnpm outdated` should work when the package tarballs are hosted on a domain that differs from the registry's domain [#5492](https://github.com/pnpm/pnpm/issues/5492).
- `strict-peer-dependencies` is set to `false` by default.

## 7.13.4

### Patch Changes

- `pnpm link <pkg> --global` should work when a custom target directory is specified with the `--dir` CLI option [#5473](https://github.com/pnpm/pnpm/pull/5473).
- It should be possible to override dependencies with local packages using overrides [#5443](https://github.com/pnpm/pnpm/issues/5443).

## 7.13.3

### Patch Changes

- Don't crash when `auto-install-peers` is set to `true` and installation is done on a workspace with that has the same dependencies in multiple projects [#5454](https://github.com/pnpm/pnpm/issues/5454).
- Add global option in `pnpm link --help` [#5461](https://github.com/pnpm/pnpm/pull/5461).
- Show execution time on `install`, `update`, `add`, and `remove` [#1021](https://github.com/pnpm/pnpm/issues/1021).
- Fix the return path of `pnpm pack`, when a custom destination directory is used [#5471](https://github.com/pnpm/pnpm/issues/5471).

## 7.13.2

### Patch Changes

- When linking commands to a directory, remove any .exe files that are already present in that target directory by the same name.

  This fixes an issue with pnpm global update on Windows. If pnpm was installed with the standalone script and then updated with pnpm using `pnpm add --global pnpm`, the exe file initially created by the standalone script should be removed.

- When a direct dependency fails to resolve, print the path to the project directory in the error message.
- `pnpm patch-commit` should work when the patch directory is specified with a trailing slash [#5449](https://github.com/pnpm/pnpm/issues/5449).

## 7.13.1

### Patch Changes

- `pnpm update --interactive` should not list dependencies ignored via the `pnpm.updateConfig.ignoreDependencies` setting.

## 7.13.0

### Minor Changes

- It is possible now to update all dependencies except the listed ones using `!`. For instance, update all dependencies, except `lodash`:

  ```
  pnpm update !lodash
  ```

  It also works with pattends, for instance:

  ```
  pnpm update !@babel/*
  ```

  And it may be combined with other patterns:

  ```
  pnpm update @babel/* !@babel/core
  ```

### Patch Changes

- Hooks should be applied on `pnpm deploy` [#5306](https://github.com/pnpm/pnpm/issues/5306).
- Stop `--filter-prod` option to run command on all the projects when used on workspace. `--filter-prod` option now only filter from `dependencies` and omit `devDependencies` instead of including all the packages when used on workspace. So what was happening is that if you use `--filter-prod` on workspace root like this:
  ```bash
  pnpm --filter-prod ...build-modules exec node -e 'console.log(require(`./package.json`).name)'
  ```
  it was printing all the package of workspace, where it should only print the package name of itself and packages where it has been added as `dependency` (not as `devDependencies`)
- Don't override the root dependency when auto installing peer dependencies [#5412](https://github.com/pnpm/pnpm/issues/5412).

## 7.12.2

### Patch Changes

- Don't crash when auto-install-peers is true and the project has many complex circular dependencies [#5394](https://github.com/pnpm/pnpm/pull/5394).
- `pnpm link --global` should work with the `--dir=<path>` option [#5371](https://github.com/pnpm/pnpm/pull/5371).

## 7.12.1

### Patch Changes

- Deduplicate peer dependencies when automatically installing them [#5373](https://github.com/pnpm/pnpm/issues/5373).

## 7.12.0

### Minor Changes

- A new setting supported in the pnpm section of the `package.json` file: `allowNonAppliedPatches`. When it is set to `true`, non-applied patches will not cause an error, just a warning will be printed. For example:

  ```json
  {
    "name": "foo",
    "version": "1.0.0",
    "pnpm": {
      "patchedDependencies": {
        "express@4.18.1": "patches/express@4.18.1.patch"
      },
      "allowNonAppliedPatches": true
    }
  }
  ```

- Now it is possible to exclude packages from hoisting by prepending a `!` to the pattern. This works with both the `hoist-pattern` and `public-hoist-pattern` settings. For instance:

  ```
  public-hoist-pattern[]='*types*'
  public-hoist-pattern[]='!@types/react'

  hoist-pattern[]='*eslint*'
  hoist-pattern[]='!*eslint-plugin*'
  ```

  Ref [#5272](https://github.com/pnpm/pnpm/issues/5272)

### Patch Changes

- When the same dependency with missing peers is used in multiple workspace projects, install the missing peers in each workspace project [#4820](https://github.com/pnpm/pnpm/issues/4820).
- `pnpm patch` should work on files that don't have an end of line [#5320](https://github.com/pnpm/pnpm/issues/5320).
- Fix `pnpm patch` using a custom `--edit-dir`.

## 7.11.0

### Minor Changes

- `pnpm patch`: edit the patched package in a directory specified by the `--edit-dir` option. E.g., `pnpm patch express@3.1.0 --edit-dir=/home/xxx/src/patched-express` [#5304](https://github.com/pnpm/pnpm/pull/5304)

### Patch Changes

- Auto installing a peer dependency in a workspace that also has it as a dev dependency in another project [#5144](https://github.com/pnpm/pnpm/issues/5144).
- When an error happens during installation of a subdependency, print some context information in order to be able to locate that subdependency. Print the exact chain of packages that led to the problematic dependency.

## 7.10.0

### Minor Changes

- New time-based resolution strategy supported.

  When `resolution-mode` is set to `time-based`, pnpm will resolve dependencies the following way:

  1. Direct dependencies will be resolved to their lowest versions. So if there is `foo@^1.1.0` in the dependencies, then `1.1.0` will be installed.
  2. Subdependencies will be resolved from versions that were published before the last direct dependency was published.

  With this resolution mode installations with hot cache are faster. It also reduces the chance of subdependency hijacking as subdependencies will be updated only if direct dependencies are updated.

  This resolution mode works only with npm's [full metadata](https://github.com/npm/registry/blob/master/docs/responses/package-metadata.md#full-metadata-format). So it is slower in some scenarios. However, if you use [Verdaccio](https://verdaccio.org/) v5.15.1 or newer, you may set the `registry-supports-time-field` setting to `true`, and it will be really fast.

  Related [RFC](https://github.com/pnpm/rfcs/pull/2).

- Enhance `pnpm env` with the `remove` command. To remove a Node.js version installed by pnpm, run:

  ```
  pnpm env remove --global <node.js version>
  ```

### Patch Changes

- `pnpm store prune` should remove all cached metadata.
- Don't modify the manifest of the injected workspace project, when it has the same dependency in prod and peer dependencies.

## 7.9.5

### Patch Changes

- Set `NODE_PATH` when `prefer-symlinked-executables` is enabled [#5251](https://github.com/pnpm/pnpm/pull/5251).
- Fail with a meaningful error when the audit endpoint doesn't exist [#5200](https://github.com/pnpm/pnpm/issues/5200).
- Symlink a local dependency to `node_modules`, even if the target directory doesn't exist [#5219](https://github.com/pnpm/pnpm/issues/5219).

## 7.9.4

### Patch Changes

- Auto install peer dependencies when auto-install-peers is set to true and the lockfile is up to date [#5213](https://github.com/pnpm/pnpm/issues/5213).
- `pnpm env`: for Node.js<16 install the x64 build on Darwin ARM as ARM build is not available [#5239](https://github.com/pnpm/pnpm/pull/5239).
- `pnpm env`: log a message when the node.js tarball starts the download [#5241](https://github.com/pnpm/pnpm/pull/5241).
- Fix `pnpm install --merge-git-branch-lockfile` when the lockfile is up to date [#5212](https://github.com/pnpm/pnpm/issues/5212).

## 7.9.4-0

### Patch Changes

- extend cafs with getFilePathByModeInCafs [#5232](https://github.com/pnpm/pnpm/pull/5232).

## 7.9.3

### Patch Changes

- Remove legacy signal handlers [#5224](https://github.com/pnpm/pnpm/pull/5224)

## 7.9.2

### Patch Changes

- When the same package is both in "peerDependencies" and in "dependencies", treat this dependency as a peer dependency if it may be resolved from the dependencies of parent packages [#5210](https://github.com/pnpm/pnpm/pull/5210).
- Update node-gyp to v9.
- Update the [compatibility database](https://www.npmjs.com/package/@yarnpkg/extensions).

## 7.9.1

### Patch Changes

- `pnpm setup`: don't use `setx` to set env variables on Windows.

## 7.9.0

### Minor Changes

- When `ignore-dep-scripts` is `true`, ignore scripts of dependencies but run the scripts of the project.
- Support a new hook for passing a custom package importer to the store controller.
- When `ignore-compatibility-db` is set to `true`, the [compatibility database](https://github.com/yarnpkg/berry/blob/master/packages/yarnpkg-extensions/sources/index.ts) will not be used to patch dependencies [#5132](https://github.com/pnpm/pnpm/issues/5132).
- Print the versions of packages in peer dependency warnings and errors.

### Patch Changes

- Don't print the same deprecation warning multiple times.
- On POSIX `pnpm setup` should suggest users to source the config instead of restarting the terminal.
- Installing a package with `bin` that points to an `.exe` file on Windows [#5159](https://github.com/pnpm/pnpm/issues/5159).
- Fix bug where the package manifest was not resolved if `verify-store-integrity` is set to `false`.
- Fix sorting of keys in lockfile to make it more deterministic and prevent unnecessary churn in the lockfile [#5151](https://github.com/pnpm/pnpm/pull/5151).
- Don't create a separate bundle for pnpx.

## 7.8.0

### Minor Changes

- When `publishConfig.directory` is set, only symlink it to other workspace projects if `publishConfig.linkDirectory` is set to `true`. Otherwise, only use it for publishing [#5115](https://github.com/pnpm/pnpm/issues/5115).

### Patch Changes

- Don't incorrectly identify a lockfile out-of-date when the package has a publishConfig.directory field [#5124](https://github.com/pnpm/pnpm/issues/5124).
- Don't crash when a config file contains a setting with an env variable that doesn't exist [#5093](https://github.com/pnpm/pnpm/issues/5093).

## 7.7.1

### Patch Changes

- pnpm should not consider a lockfile out-of-date if `auto-install-peers` is set to `true` and the peer dependency is in `devDependencies` or `optionalDependencies` [#5080](https://github.com/pnpm/pnpm/issues/5080).
- Don't incorrectly consider a lockfile out-of-date when `workspace:^` or `workspace:~` version specs are used in a workspace.

## 7.7.0

### Minor Changes

- Add experimental lockfile format that should merge conflict less in the `importers` section. Enabled by setting the `use-inline-specifiers-lockfile-format = true` feature flag in `.npmrc`.

  If this feature flag is committed to a repo, we recommend setting the minimum allowed version of pnpm to this release in the `package.json` `engines` field. Once this is set, older pnpm versions will throw on invalid lockfile versions.

- Add `publishDirectory` field to the lockfile and relink the project when it changes.

- `verify-store-integrity=false` makes pnpm skip checking the integrities of files in the global content-addressable store.

- Allow to set `only-built-dependencies[]` through `.npmrc`.

### Patch Changes

- It should be possible to publish a package with local dependencies from a custom publish directory (set via `publishConfig.directory`) [#3901](https://github.com/pnpm/pnpm/issues/3901#issuecomment-1194156886).
- `pnpm deploy` should inject local dependencies of all types (dependencies, optionalDependencies, devDependencies) [#5078](https://github.com/pnpm/pnpm/issues/5078).
- When a project in a workspace has a `publishConfig.directory` set, dependent projects should install the project from that directory [#3901](https://github.com/pnpm/pnpm/issues/3901)
- **pnpm deploy**: accept absolute paths and use cwd instead of workspaceDir for deploy target directory [#4980](https://github.com/pnpm/pnpm/issues/4980).
- **pnpm setup** should update `.zshrc` in the right directory when a `$ZDOTDIR` is set.

## 7.6.0

### Minor Changes

- A new setting supported: `prefer-symlinked-executables`. When `true`, pnpm will create symlinks to executables in
  `node_modules/.bin` instead of command shims (but on POSIX systems only).

  This setting is `true` by default when `node-linker` is set to `hoisted`.

  Related issue: [#4782](https://github.com/pnpm/pnpm/issues/4782).

- When `lockfile-include-tarball-url` is set to `true`, every entry in `pnpm-lock.yaml` will contain the full URL to the package's tarball [#5054](https://github.com/pnpm/pnpm/pull/5054).

### Patch Changes

- `pnpm deploy` should include all dependencies by default [#5035](https://github.com/pnpm/pnpm/issues/5035).
- Don't print warnings about file verifications. Just print info messages instead.
- `pnpm publish --help` should print the `--recursive` and `--filter` options [#5019](https://github.com/pnpm/pnpm/issues/5019).
- It should be possible to run exec/run/dlx with the `--use-node-version` option.
- `pnpm deploy` should not modify the lockfile [#5071](https://github.com/pnpm/pnpm/issues/5071)
- `pnpm deploy` should not fail in CI [#5071](https://github.com/pnpm/pnpm/issues/5071)
- When `auto-install-peers` is set to `true`, automatically install direct peer dependencies [#5028](https://github.com/pnpm/pnpm/pull/5067).

  So if your project the next manifest:

  ```json
  {
    "dependencies": {
      "lodash": "^4.17.21"
    },
    "peerDependencies": {
      "react": "^18.2.0"
    }
  }
  ```

  pnpm will install both lodash and react as a regular dependencies.

## 7.5.2

### Patch Changes

- Don't print any info messages about .pnpmfile.cjs [#5027](https://github.com/pnpm/pnpm/issues/5027).
- Do not print a package with unchanged version in the installation summary [#5031](https://github.com/pnpm/pnpm/pull/5031).

## 7.5.1

### Patch Changes

- Don't symlink the autoinstalled peer dependencies to the root of `node_modules` [#4988](https://github.com/pnpm/pnpm/issues/4988).
- Avoid retaining a copy of the contents of files deleted during patching [#5003](https://github.com/pnpm/pnpm/issues/5003).
- Remove file reporter logging. Logged file is not useful [#4949](https://github.com/pnpm/pnpm/issues/4949).

## 7.5.0

### Minor Changes

- A new value `rolling` for option `save-workspace-protocol`. When selected, pnpm will save workspace versions using a rolling alias (e.g. `"foo": "workspace:^"`) instead of pinning the current version number (e.g. `"foo": "workspace:^1.0.0"`). Usage example, in the root of your workspace, create a `.npmrc` with the following content:

  ```
  save-workspace-protocol=rolling
  ```

### Patch Changes

- `pnpm remove <pkg>` should not fail in a workspace that has patches [#4954](https://github.com/pnpm/pnpm/issues/4954#issuecomment-1172858634)
- The hash of the patch file should be the same on both Windows and POSIX [#4961](https://github.com/pnpm/pnpm/issues/4961).
- `pnpm env use` should throw an error on a system that use the MUSL libc.

## 7.4.1

### Patch Changes

- `pnpm install` in a workspace with patches should not fail when doing partial installation [#4954](https://github.com/pnpm/pnpm/issues/4954).
- Never skip lockfile resolution when the lockfile is not up-to-date and `--lockfile-only` is used. Even if `frozen-lockfile` is `true` [#4951](https://github.com/pnpm/pnpm/issues/4951).
- Never add an empty `patchedDependencies` field to `pnpm-lock.yaml`.

## 7.4.0

### Minor Changes

- Dependencies patching is possible via the `pnpm.patchedDependencies` field of the `package.json`.
  To patch a package, the package name, exact version, and the relative path to the patch file should be specified. For instance:

  ```json
  {
    "pnpm": {
      "patchedDependencies": {
        "eslint@1.0.0": "./patches/eslint@1.0.0.patch"
      }
    }
  }
  ```

- Two new commands added: `pnpm patch` and `pnpm patch-commit`.

  `pnpm patch <pkg>` prepares a package for patching. For instance, if you want to patch express v1, run:

  ```
  pnpm patch express@1.0.0
  ```

  pnpm will create a temporary directory with `express@1.0.0` that you can modify with your changes.
  Once you are read with your changes, run:

  ```
  pnpm patch-commit <path to temp folder>
  ```

  This will create a patch file and write it to `<project>/patches/express@1.0.0.patch`.
  Also, it will reference this new patch file from the `patchedDependencies` field in `package.json`:

  ```json
  {
    "pnpm": {
      "patchedDependencies": {
        "express@1.0.0": "patches/express@1.0.0.patch"
      }
    }
  }
  ```

- A new experimental command added: `pnpm deploy`. The deploy command takes copies a project from a workspace and installs all of its production dependencies (even if some of those dependencies are other projects from the workspace).

  For example, the new command will deploy the project named `foo` to the `dist` directory in the root of the workspace:

  ```
  pnpm --filter=foo deploy dist
  ```

- `package-import-method` supports a new option: `clone-or-copy`.

- New setting added: `include-workspace-root`. When it is set to `true`, the `run`, `exec`, `add`, and `test` commands will include the root package, when executed recursively [#4906](https://github.com/pnpm/pnpm/issues/4906)

### Patch Changes

- Don't crash when `pnpm update --interactive` is cancelled with Ctrl+c.
- The `use-node-version` setting should work with prerelease Node.js versions. For instance:

  ```
  use-node-version=18.0.0-rc.3
  ```

- Return early when the lockfile is up-to-date.
- Resolve native workspace path for case-insensitive file systems [#4904](https://github.com/pnpm/pnpm/issues/4904).
- Don't link local dev dependencies, when prod dependencies should only be installed.
- `pnpm audit --fix` should not add an override for a vulnerable package that has no fixes released.
- Update the compatibility database.

## 7.3.0

### Minor Changes

- A new setting added: `pnpm.peerDependencyRules.allowAny`. `allowAny` is an array of package name patterns, any peer dependency matching the pattern will be resolved from any version, regardless of the range specified in `peerDependencies`. For instance:

  ```
  {
    "pnpm": {
      "peerDependencyRules": {
        "allowAny": ["@babel/*", "eslint"]
      }
    }
  }
  ```

  The above setting will mute any warnings about peer dependency version mismatches related to `@babel/` packages or `eslint`.

- The `pnpm.peerDependencyRules.ignoreMissing` setting may accept package name patterns. So you may ignore any missing `@babel/*` peer dependencies, for instance:

  ```json
  {
    "pnpm": {
      "peerDependencyRules": {
        "ignoreMissing": ["@babel/*"]
      }
    }
  }
  ```

- **Experimental.** New settings added: `git-branch-lockfile`, `merge-git-branch-lockfiles`, `merge-git-branch-lockfiles-branch-pattern` [#4475](https://github.com/pnpm/pnpm/pull/4475).

### Patch Changes

- Packages that should be built are always cloned or copied from the store. This is required to prevent the postinstall scripts from modifying the original source files of the package.

## 7.2.1

### Patch Changes

- Support Node.js from v14.6.
- Don't fail when the cafile setting is specified [#4877](https://github.com/pnpm/pnpm/issues/4877). This fixes a regression introduced in pnpm v7.2.0.

## 7.2.0

### Minor Changes

- A new setting is supported for ignoring specific deprecation messages: `pnpm.allowedDeprecatedVersions`. The setting should be provided in the `pnpm` section of the root `package.json` file. The below example will mute any deprecation warnings about the `request` package and warnings about `express` v1:

  ```json
  {
    "pnpm": {
      "allowedDeprecatedVersions": {
        "request": "*",
        "express": "1"
      }
    }
  }
  ```

  Related issue: [#4306](https://github.com/pnpm/pnpm/issues/4306)
  Related PR: [#4864](https://github.com/pnpm/pnpm/pull/4864)

### Patch Changes

- Update the compatibility database.
- Report only the first occurrence of a deprecated package.
- Add better hints to the peer dependency issue errors.

## 7.1.9

### Patch Changes

- When the same package is found several times in the dependency graph, correctly autoinstall its missing peer dependencies at all times [#4820](https://github.com/pnpm/pnpm/issues/4820).

## 7.1.8

### Patch Changes

- Suggest updating using Corepack, when pnpm was installed via Corepack.
- It should be possible to install a git-hosted package that has no `package.json` file [#4822](https://github.com/pnpm/pnpm/issues/4822).
- Fix pre-compiled pnpm binaries crashing when NODE_MODULES is set.

## 7.1.7

### Patch Changes

- Improve the performance of the build sequence calculation step [#4815](https://github.com/pnpm/pnpm/pull/4815).
- Correctly detect repeated dependency sequence during resolution [#4813](https://github.com/pnpm/pnpm/pull/4813).

## 7.1.6

### Patch Changes

- Don't fail on projects with linked dependencies, when `auto-install-peers` is set to `true` [#4796](https://github.com/pnpm/pnpm/issues/4796).
- `NODE_ENV=production pnpm install --dev` should only install dev deps [#4745](https://github.com/pnpm/pnpm/pull/4745).

## 7.1.5

### Patch Changes

- Correctly detect the active Node.js version, when the pnpm CLI is bundled to an executable [#4203](https://github.com/pnpm/pnpm/issues/4203).

## 7.1.3

### Patch Changes

- When `auto-install-peers` is set to `true`, automatically install missing peer dependencies without writing them to `package.json` as dependencies. This makes pnpm handle peer dependencies the same way as npm v7 [#4776](https://github.com/pnpm/pnpm/pull/4776).

## 7.1.2

### Patch Changes

- `pnpm setup` should not fail on Windows if `PNPM_HOME` is not yet in the system registry [#4757](https://github.com/pnpm/pnpm/issues/4757)
- `pnpm dlx` shouldn't modify the lockfile in the current working directory [#4743](https://github.com/pnpm/pnpm/issues/4743).

## 7.1.1

### Patch Changes

- When the global bin directory is set to a symlink, check not only the symlink in the PATH but also the target of the symlink [#4744](https://github.com/pnpm/pnpm/issues/4744).
- Sanitize the directory names created inside `node_modules/.pnpm` and inside the global store [#4716](https://github.com/pnpm/pnpm/issues/4716)
- All arguments after `pnpm create <pkg>` should be passed to the executed create app package. So `pnpm create next-app --typescript` should work`.
- Resolve commits from GitHub via https [#4734](https://github.com/pnpm/pnpm/pull/4734).

## 7.1.0

### Minor Changes

- Added support for `libc` field in `package.json` [#4454](https://github.com/pnpm/pnpm/issues/4454).

### Patch Changes

- `pnpm setup` should update the config of the current shell, not the preferred shell.
- `pnpm setup` should not override the PNPM_HOME env variable, unless `--force` is used.
- `pnpm dlx` should print messages about installation to stderr [#1698](https://github.com/pnpm/pnpm/issues/1698).
- `pnpm dlx` should work with git-hosted packages. For example: `pnpm dlx gengjiawen/envinfo` [#4714](https://github.com/pnpm/pnpm/issues/4714).
- `pnpm run --stream` should prefix the output with directory [#4702](https://github.com/pnpm/pnpm/issues/4702)

## 7.0.1

### Patch Changes

- Use Yarn's compatibility database to patch broken packages in the ecosystem with package extensions [#4676](https://github.com/pnpm/pnpm/pull/4676).
- `pnpm dlx` should work when the bin name of the executed package isn't the same as the package name [#4672](https://github.com/pnpm/pnpm/issues/4672).
- Throw an error if arguments are passed to the `pnpm init` command [#4665](https://github.com/pnpm/pnpm/pull/4665).
- `pnpm prune` works in a workspace [#4647](https://github.com/pnpm/pnpm/pull/4691).
- Do not report request retry warnings when loglevel is set to `error` [#4669](https://github.com/pnpm/pnpm/issues/4669).
- `pnpm prune` does not remove hoisted dependencies [#4647](https://github.com/pnpm/pnpm/pull/4691).

## 7.0.0

### Major Changes

- Node.js 12 is not supported.

- The root package is excluded by default, when running `pnpm -r exec|run|add` [#2769](https://github.com/pnpm/pnpm/issues/2769).
- Filtering by path is done by globs.

  In pnpm v6, in order to pick packages under a certain directory, the following filter was used: `--filter=./apps`

  In pnpm v7, a glob should be used: `--filter=./apps/**`

  For easier upgrade, we have also added a setting to turn back filtering as it was in v6. Just set `legacy-dir-filtering=true` in `.npmrc`.

- The `NODE_PATH` env variable is not set in the command shims (the files in `node_modules/.bin`). This env variable was really long and frequently caused errors on Windows.

  Also, the `extend-node-path` setting is removed.

  Related PR: [#4253](https://github.com/pnpm/pnpm/pull/4253)

- The `embed-readme` setting is `false` by default.
- When using `pnpm run <script>`, all command line arguments after the script name are now passed to the script's argv, even `--`. For example, `pnpm run echo --hello -- world` will now pass `--hello -- world` to the `echo` script's argv. Previously flagged arguments (e.g. `--silent`) were interpreted as pnpm arguments unless `--` came before it.
- Side effects cache is turned on by default. To turn it off, use `side-effects-cache=false`.
- The `npm_config_argv` env variable is not set for scripts [#4153](https://github.com/pnpm/pnpm/discussions/4153).
- `pnpx` is now just an alias of `pnpm dlx`.

  If you want to just execute the command of a dependency, run `pnpm <cmd>`. For instance, `pnpm eslint`.

  If you want to install and execute, use `pnpm dlx <pkg name>`.

- `pnpm install -g pkg` will add the global command only to a predefined location. pnpm will not try to add a bin to the global Node.js or npm folder. To set the global bin directory, either set the `PNPM_HOME` env variable or the [`global-bin-dir`](https://pnpm.io/npmrc#global-bin-dir) setting.
- `pnpm pack` should only pack a file as an executable if it's a bin or listed in the `publishConfig.executableFiles` array.
- `-W` is not an alias of `--ignore-workspace-root-check` anymore. Just use `-w` or `--workspace-root` instead, which will also allow to install dependencies in the root of the workspace.
- Allow to execute a lifecycle script in a directory that doesn't match the package's name. Previously this was only allowed with the `--unsafe-perm` CLI option [#3709](https://github.com/pnpm/pnpm/issues/3709).

- Local dependencies referenced through the `file:` protocol are hard linked (not symlinked) [#4408](https://github.com/pnpm/pnpm/pull/4408). If you need to symlink a dependency, use the `link:` protocol instead.

- `strict-peer-dependencies` is `true` by default [#4427](https://github.com/pnpm/pnpm/pull/4427).

- A prerelease version is always added as an exact version to `package.json`. If the `next` version of `foo` is `1.0.0-beta.1` then running `pnpm add foo@next` will add this to `package.json`:

  ```json
  {
    "dependencies": {
      "foo": "1.0.0-beta.1"
    }
  }
  ```

  PR: [#4435](https://github.com/pnpm/pnpm/pull/4435)

- Dependencies of the root workspace project are not used to resolve peer dependencies of other workspace projects [#4469](https://github.com/pnpm/pnpm/pull/4469).

- Don't hoist types by default to the root of `node_modules` [#4459](https://github.com/pnpm/pnpm/pull/4459).

- Any package with "prettier" in its name is hoisted.

- Changed the location of the global store from `~/.pnpm-store` to `<pnpm home directory>/store`

  On Linux, by default it will be `~/.local/share/pnpm/store`
  On Windows: `%LOCALAPPDATA%/pnpm/store`
  On macOS: `~/Library/pnpm/store`

  Related issue: [#2574](https://github.com/pnpm/pnpm/issues/2574)

- 4bed585e2: The next deprecated settings were removed:

  - frozen-shrinkwrap
  - prefer-frozen-shrinkwrap
  - shared-workspace-shrinkwrap
  - shrinkwrap-directory
  - lockfile-directory
  - shrinkwrap-only
  - store

- Use a base32 hash instead of a hex to encode too long dependency paths inside `node_modules/.pnpm` [#4552](https://github.com/pnpm/pnpm/pull/4552).

- New setting added: `git-shallow-hosts`. When cloning repositories from "shallow-hosts", pnpm will use shallow cloning to fetch only the needed commit, not all the history [#4548](https://github.com/pnpm/pnpm/pull/4548).

- Lockfile version bumped to v5.4.

- Exit with an error when running `pnpm install` in a directory that has no `package.json` file in it (and in parent directories) [#4609](https://github.com/pnpm/pnpm/issues/4609).

## 6.32.11

### Patch Changes

- `pnpm publish` should work correctly in a workspace, when the latest npm CLI is installed [#4348](https://github.com/pnpm/pnpm/issues/4348).
- Installation shouldn't fail when a package from node_modules is moved to the `node_modules/.ignored` subfolder and a package with that name is already present in `node_modules/.ignored' [#4626](https://github.com/pnpm/pnpm/pull/4626).

## 6.32.10

### Patch Changes

- It should be possible to use a chain of local file dependencies [#4611](https://github.com/pnpm/pnpm/issues/4611).
- Filtering by directory should work with directories that have unicode chars in the name [#4595](https://github.com/pnpm/pnpm/pull/4595).

## 6.32.9

### Patch Changes

- Fix an error with peer resolutions, which was happening when there was a circular dependency and another dependency that had the name of the circular dependency as a substring.
- When `pnpm exec` is running a command in a workspace project, the commands that are in the dependencies of that workspace project should be in the PATH [#4481](https://github.com/pnpm/pnpm/issues/4481).
- Hide "WARN deprecated" messages on loglevel error [#4507](https://github.com/pnpm/pnpm/pull/4507)

  Don't show the progress bar when loglevel is set to warn or error.

## 6.32.8

### Patch Changes

- Don't check the integrity of the store with the package version from the lockfile, when the package was updated [#4580](https://github.com/pnpm/pnpm/pull/4580).
- Don't update a direct dependency that has the same name as a dependency in the workspace, when adding a new dependency to a workspace project [#4575](https://github.com/pnpm/pnpm/pull/4575).

## 6.32.7

### Patch Changes

- Setting the `auto-install-peers` to `true` should work.

## 6.32.6

### Patch Changes

- Linked in dependencies should be considered when resolving peer dependencies [#4541](https://github.com/pnpm/pnpm/pull/4541).
- Peer dependency should be correctly resolved from the workspace, when it is declared using a workspace protocol [#4529](https://github.com/pnpm/pnpm/issues/4529).

## 6.32.5

### Patch Changes

- `dependenciesMeta` should be saved into the lockfile, when it is added to the package manifest by a hook.

## 6.32.4

### Patch Changes

- Show a friendly error message when it is impossible to get the current Git branch name during publish [#4488](https://github.com/pnpm/pnpm/pull/4488).
- When checking if the lockfile is up-to-date, an empty `dependenciesMeta` field in the manifest should be satisfied by a not set field in the lockfile [#4463](https://github.com/pnpm/pnpm/pull/4463).
- It should be possible to reference a workspace project that has no version specified in its `package.json` [#4487](https://github.com/pnpm/pnpm/pull/4487).

## 6.32.3

### Patch Changes

- 4941f31ee: The location of an injected directory dependency should be correctly located, when there is a chain of local dependencies (declared via the `file:` protocol`).

  The next scenario was not working prior to the fix. There are 3 projects in the same folder: foo, bar, qar.

  `foo/package.json`:

  ```json
  {
    "name": "foo",
    "dependencies": {
      "bar": "file:../bar"
    },
    "dependenciesMeta": {
      "bar": {
        "injected": true
      }
    }
  }
  ```

  `bar/package.json`:

  ```json
  {
    "name": "bar",
    "dependencies": {
      "qar": "file:../qar"
    },
    "dependenciesMeta": {
      "qar": {
        "injected": true
      }
    }
  }
  ```

  `qar/package.json`:

  ```json
  {
    "name": "qar"
  }
  ```

  Related PR: [#4415](https://github.com/pnpm/pnpm/pull/4415).

## 6.32.2

### Patch Changes

- In order to guarantee that only correct data is written to the store, data from the lockfile should not be written to the store. Only data directly from the package tarball or package metadata [#4395](https://github.com/pnpm/pnpm/pull/4395).
- Throw a meaningful error message on `pnpm install` when the lockfile is broken and `node-linker` is set to `hoisted` [#4387](https://github.com/pnpm/pnpm/pull/4387).

## 6.32.1

### Patch Changes

- `onlyBuiltDependencies` should work [#4377](https://github.com/pnpm/pnpm/pull/4377). The new `onlyBuiltDependencies` feature was released with a bug in v6.32.0, so it didn't work.

## 6.32.0

### Minor Changes

- A new setting is supported in the `pnpm` section of the `package.json` file [#4001](https://github.com/pnpm/pnpm/issues/4001). `onlyBuiltDependencies` is an array of package names that are allowed to be executed during installation. If this field exists, only mentioned packages will be able to run install scripts.

  ```json
  {
    "pnpm": {
      "onlyBuiltDependencies": ["fsevents"]
    }
  }
  ```

- `-F` is a short alias of `--filter` [#3467](https://github.com/pnpm/pnpm/issues/3467).

- When adding a new dependency, use the version specifier from the overrides, when present [#4313](https://github.com/pnpm/pnpm/issues/4313).

  Normally, if the latest version of `foo` is `2.0.0`, then `pnpm add foo` installs `foo@^2.0.0`. This behavior changes if `foo` is specified in an override:

  ```json
  {
    "pnpm": {
      "overrides": {
        "foo": "1.0.0"
      }
    }
  }
  ```

  In this case, `pnpm add foo` will add `foo@1.0.0` to the dependency. However, if a version is explicitly specifying, then the specified version will be used and the override will be ignored. So `pnpm add foo@0` will install v0 and it doesn't matter what is in the overrides.

### Patch Changes

- Ignore case, when verifying package name in the store [#4367](https://github.com/pnpm/pnpm/issues/4367).
- When a peer dependency range is extended with `*`, just replace any range with `*`.
- When some dependency types are skipped, let the user know via the installation summary.

## 6.31.0

### Minor Changes

- Added `--shell-mode`/`-c` option support to `pnpm exec` [#4328](https://github.com/pnpm/pnpm/pull/4328)

  - `--shell-mode`: shell interpreter. See: https://github.com/sindresorhus/execa/tree/484f28de7c35da5150155e7a523cbb20de161a4f#shell

  Usage example:

  ```shell
  pnpm -r --shell-mode exec -- echo \"\$PNPM_PACKAGE_NAME\"
  pnpm -r -c exec -- echo \"\$PNPM_PACKAGE_NAME\"
  ```

  ```json
  {
    "scripts": {
      "check": " pnpm -r --shell-mode exec -- echo \"\\$PNPM_PACKAGE_NAME\""
    }
  }
  ```

### Patch Changes

- Remove meaningless keys from `publishConfig` when the `pack` or `publish` commands are used [#4311](https://github.com/pnpm/pnpm/issues/4311)
- The `pnpx`, `pnpm dlx`, `pnpm create`, and `pnpm exec` commands should set the `npm_config_user_agent` env variable [#3985](https://github.com/pnpm/pnpm/issues/3985).

## 6.30.1

### Patch Changes

- This fixes an issue introduced in pnpm v6.30.0.

  When a package is not linked to `node_modules`, no info message should be printed about it being "relinked" from the store [#4314](https://github.com/pnpm/pnpm/issues/4314).

## 6.30.0

### Minor Changes

- When checking that a package is linked from the store, check the existence of the package and read its stats with a single filesystem operation [#4304](https://github.com/pnpm/pnpm/pull/4304).

## 6.29.2

### Patch Changes

- `node_modules` directories inside injected dependencies should not be overwritten [#4299](https://github.com/pnpm/pnpm/pull/4299).

## 6.29.1

### Patch Changes

- Installation should not hang when there are broken symlinks in `node_modules`.

## 6.29.0

### Minor Changes

- Add support of the `update-notifier` configuration option [#4158](https://github.com/pnpm/pnpm/issues/4158).

### Patch Changes

- A package should be able to be a dependency of itself.

## 6.28.0

### Minor Changes

- New option added: `embed-readme`. When `false`, `pnpm publish` doesn't save the readme file's content to `package.json` before publish [#4265](https://github.com/pnpm/pnpm/pull/4265).

### Patch Changes

- `pnpm exec` should look for the executed command in the `node_modules/.bin` directory that is relative to the current working directory. Only after that should it look for the executable in the workspace root.

## 6.27.2

### Patch Changes

- [Injected dependencies](https://pnpm.io/package_json#dependenciesmetainjected) should work properly in projects that use the hoisted node linker [#4259](https://github.com/pnpm/pnpm/pull/4259).

## 6.27.1

### Patch Changes

- `peerDependencyRules` should work when both `overrides` and `packageExtensions` are present as well [#4255](https://github.com/pnpm/pnpm/pull/4255).
- `pnpm list` should show information whether a package is private or not [#4246](https://github.com/pnpm/pnpm/issues/4246).

## 6.27.0

### Minor Changes

- Side effects cache is not an experimental feature anymore.

  Side effects cache is saved separately for packages with different dependencies. So if `foo` has `bar` in the dependencies, then a separate cache will be created each time `foo` is installed with a different version of `bar` [#4238](https://github.com/pnpm/pnpm/pull/4238).

### Patch Changes

- Update command should work when there is a dependency with empty version in `devDependencies` [#4196](https://github.com/pnpm/pnpm/issues/4196).
- Side effects cache should work in a workspace.

## 6.26.1

### Patch Changes

- During installation, override any symlinks in `node_modules`. This was an issue only with `node-linker=hoisted` [#4229](https://github.com/pnpm/pnpm/pull/4229).
- Print warnings about deprecated subdependencies [#4227](https://github.com/pnpm/pnpm/issues/4227).

## 6.26.0

### Minor Changes

- In order to mute some types of peer dependency warnings, a new section in `package.json` may be used for declaring peer dependency warning rules. For example, the next configuration will turn off any warnings about missing `babel-loader` peer dependency and about `@angular/common`, when the wanted version of `@angular/common` is not v13.

  ```json
  {
    "name": "foo",
    "version": "0.0.0",
    "pnpm": {
      "peerDependencyRules": {
        "ignoreMissing": ["babel-loader"],
        "allowedVersions": {
          "@angular/common": "13"
        }
      }
    }
  }
  ```

- New setting supported: `auto-install-peers`. When it is set to `true`, `pnpm add <pkg>` automatically installs any missing peer dependencies as `devDependencies` [#4213](https://github.com/pnpm/pnpm/pull/4213).

## 6.25.1

### Patch Changes

- Run the install scripts of hoisted dependencies in a workspace with no root project [#4209](https://github.com/pnpm/pnpm/issues/4209).

## 6.25.0

### Minor Changes

- New installation mode added that creates a flat `node_modules` directory without the usage of symlinks. This is similar to the one created by npm and Yarn Classic.

  To use this new installation mode, set the `node-linker` setting to `hoisted`. These are the supported values of `node-linker`:

  - `isolated` - the default value.
  - `hoisted` - flat `node_modules` without symlinks.
  - `pnp` - no `node_modules`. Yarn's Plug'n'Play managed by pnpm.

  Related issue: [#4073](https://github.com/pnpm/pnpm/issues/4073)

- Add support for token helper, a command line tool to obtain a token.

  A token helper is an executable, set in the user's `.npmrc` which outputs an auth token. This can be used in situations where the authToken is not a constant value, but is something that refreshes regularly, where a script or other tool can use an existing refresh token to obtain a new access token.

  The configuration for the path to the helper must be an absolute path, with no arguments. In order to be secure, it is only permitted to set this value in the user `.npmrc`, otherwise a project could place a value in a project local `.npmrc` and run arbitrary executables.

  Usage example:

  ```ini
  ; Setting a token helper for the default registry
  tokenHelper=/home/ivan/token-generator

  ; Setting a token helper for the specified registry
  //registry.corp.com:tokenHelper=/home/ivan/token-generator
  ```

  Related PRs:

  - [pnpm/credentials-by-uri#2](https://github.com/pnpm/credentials-by-uri/pull/2)
  - [#4163](https://github.com/pnpm/pnpm/pull/4163)

- New CLI option: `--ignore-workspace`. When used, pnpm ignores any workspace configuration found in the current or parent directories.

- If `use-beta-cli` is `true`, then don't set `npm_config_argv` env variable for scripts [#4175](https://github.com/pnpm/pnpm/pull/4175).

## 6.24.4

### Patch Changes

- Don't throw an error during install when the bin of a dependency points to a path that doesn't exist [#3763](https://github.com/pnpm/pnpm/issues/3763).

- When reporting unmet peer dependency issues, if the peer dependency is resolved not from a dependency installed by the user, then print the name of the parent package that has the bad peer dependency installed as a dependency.

  ![](https://i.imgur.com/0kjij22.png)

- Injected subdependencies should be hard linked as well. So if `button` is injected into `card` and `card` is injected into `page`, then both `button` and `card` should be injected into `page` [#4167](https://github.com/pnpm/pnpm/pull/4167).

## 6.24.3

### Patch Changes

- Install with `--frozen-lockfile` should not fail when the project has injected dependencies and a dedicated lockfile [#4098](https://github.com/pnpm/pnpm/issues/4098).

## 6.24.2

### Patch Changes

- If pnpm previously failed to install node when the `use-node-version` option is set, that download and install will now be re-attempted when pnpm is ran again [#4104](https://github.com/pnpm/pnpm/issues/4104).

- Don't warn about unmet peer dependency when the peer is resolved from a prerelease version [#4144](https://github.com/pnpm/pnpm/pull/4144).

  For instance, if a project has `react@*` as a peer dependency, then react `16.0.0-rc.0` should not cause a warning.

- `pnpm update pkg` should not fail if `pkg` not found as a direct dependency, unless `--depth=0` is passed as a CLI option [#4122](https://github.com/pnpm/pnpm/issues/4122).

- When printing peer dependency issues, print the "\*" range in double quotes. This will make it easier to copy the package resolutions and put them to the end of a `pnpm add` command for execution.

## 6.24.1

### Patch Changes

- If making an intersection of peer dependency ranges does not succeed, install should not crash [#4134](https://github.com/pnpm/pnpm/issues/4134).
- A new line should be between the summary about conflicting peers and non-conflicting ones.
- Always return an error message when the preparation of a package fails.
- `pnpm publish` should add the content of the `README.md` file to the `readme` field of the published package's `package.json` files [#4117](https://github.com/pnpm/pnpm/pull/4117).
- `pnpm publish` should work with the `--otp` option [#4115](https://github.com/pnpm/pnpm/pull/4115).

## 6.24.0

### Minor Changes

- Peer dependency issues are grouped and rendered in a nice hierarchy view.

  This is how the peer dependency issues were printed in previous versions:

  ![](https://i.imgur.com/CmJVb4F.png)

  This is how they are displayed in pnpm v6.24:

  ![](https://i.imgur.com/qUP7FVa.png)

- New option added for: `node-mirror:<releaseDir>` [#4083](https://github.com/pnpm/pnpm/pull/4083). The string value of this dynamic option is used as the base URL for downloading node when `use-node-version` is specified. The `<releaseDir>` portion of this argument can be any dir in `https://nodejs.org/download`. Which `<releaseDir>` dynamic config option gets selected depends on the value of `use-node-version`. If 'use-node-version' is a simple `x.x.x` version string, `<releaseDir>` becomes `release` and `node-mirror:release` is read. Defaults to `https://nodejs.org/download/<releaseDir>/`.

- 927c4a089: A new option `--aggregate-output` for `append-only` reporter is added. It aggregates lifecycle logs output for each command that is run in parallel, and only prints command logs when command is finished.

  Related discussion: [#4070](https://github.com/pnpm/pnpm/discussions/4070).

### Patch Changes

- Don't fail when the version of a package in the store is not a semver version [#4077](https://github.com/pnpm/pnpm/pull/4077).
- `pnpm store prune` should not fail if there are unexpected subdirectories in the content-addressable store [#4072](https://github.com/pnpm/pnpm/pull/4072).
- Don't make unnecessary retries when fetching Git-hosted packages [#2731](https://github.com/pnpm/pnpm/pull/2731).
- pnpm should read the auth token of a github-registry-hosted package, when the registry path contains the owner [#4034](https://github.com/pnpm/pnpm/issues/4034).

  So this should work:

  ```
  @owner:registry=https://npm.pkg.github.com/owner
  //npm.pkg.github.com/:_authToken=<token>
  ```

- When `strict-peer-dependencies` is used, don't fail on the first peer dependency issue. Print all the peer dependency issues and then stop the installation process [#4082](https://github.com/pnpm/pnpm/pull/4082).

- When sorting workspace projects, don't ignore the manifests of those that don't have a version field [#3933](https://github.com/pnpm/pnpm/issues/3933).

## 6.23.6

### Patch Changes

- Fixes a regression introduced in pnpm v6.23.3 via [#4044](https://github.com/pnpm/pnpm/pull/4044).

  The temporary directory to which the Git-hosted package is downloaded should not be removed prematurely [#4064](https://github.com/pnpm/pnpm/issues/4064).

## 6.23.5

### Patch Changes

- `pnpm audit` should work when a proxy is configured for the registry [#3755](https://github.com/pnpm/pnpm/issues/3755).
- Revert the change that was made in pnpm v6.23.2 causing a regression describe in [#4052](https://github.com/pnpm/pnpm/issues/4052).

## 6.23.4

### Patch Changes

- Non-standard tarball URL should be correctly calculated when the registry has no trailing slash in the configuration file [#4052](https://github.com/pnpm/pnpm/issues/4052). This is a regression caused introduced in v6.23.2 caused by [#4032](https://github.com/pnpm/pnpm/pull/4032).

## 6.23.3

### Patch Changes

- `pnpm import` should work with a lockfile generated by Yarn Berry [#3993](https://github.com/pnpm/pnpm/issues/3993).
- When preparation of a git-hosted package fails, do not refetch it [#4044](https://github.com/pnpm/pnpm/pull/4044).

## 6.23.2

### Patch Changes

- pnpm should read the auth token of a github-registry-hosted package, when the registry path contains the owner [#4034](https://github.com/pnpm/pnpm/issues/4034).

  So this should work:

  ```
  @owner:registry=https://npm.pkg.github.com/owner
  //npm.pkg.github.com/:_authToken=<token>
  ```

- When checking the correctness of the package data in the lockfile, don't use exact version comparison. `v1.0.0` should be considered to be the same as `1.0.0`. This fixes some edge cases when a package is published with a non-normalized version specifier in its `package.json` [#4036](https://github.com/pnpm/pnpm/pull/4036).

## 6.23.1

### Patch Changes

- `pnpm setup` should create shell rc files for pnpm path configuration if no such file exists prior [#4027](https://github.com/pnpm/pnpm/issues/4027).
- The postinstall scripts of dependencies should be executed after the root dependencies of the project are symlinked [#4018](https://github.com/pnpm/pnpm/issues/4018).
- `pnpm dlx` will now support version specifiers for packages. E.g. `pnpm dlx create-svelte@next` [#4023](https://github.com/pnpm/pnpm/issues/4023).

## 6.23.0

### Minor Changes

- New setting added: `scripts-prepend-node-path`. This setting can be `true`, `false`, or `warn-only`.

  When `true`, the path to the `node` executable with which pnpm executed is prepended to the `PATH` of the scripts.

  When `warn-only`, pnpm will print a warning if the scripts run with a `node` binary that differs from the `node` binary executing the pnpm CLI.

### Patch Changes

- The path to the `node` executable that executes pnpm should not be added to the `PATH`, when running scripts.
- `pnpm env use` should download the right Node.js tarball on Raspberry Pi [#4007](https://github.com/pnpm/pnpm/issues/4007).
- HTTP requests should be retried when the server responds with on of 408, 409, 420, 429 status codes.

## 6.22.2

### Patch Changes

- `pnpm exec` should exit with the exit code of the child process. This fixes a regression introduced in pnpm v6.20.4 via [#3951](https://github.com/pnpm/pnpm/pull/3951).
- `node-gyp` from the dependencies should be preferred over the `node-gyp` that is bundled with pnpm, when running scripts [#2135](https://github.com/pnpm/pnpm/issues/2135).
- `pnpm dlx pnpm` should not break the globally installed pnpm CLI.

## 6.22.1

### Patch Changes

- Downgrading `p-memoize` to v4.0.1. pnpm v6.22.0 started to print the next warning [#3989](https://github.com/pnpm/pnpm/issues/3989):

  ```
  (node:132923) TimeoutOverflowWarning: Infinity does not fit into a 32-bit signed integer.
  ```

## 6.22.0

### Minor Changes

- Added `--reverse` option support to `pnpm exec` [#3984](https://github.com/pnpm/pnpm/issues/3972).

  Usage example:

  ```
  pnpm --reverse -r exec pwd
  ```

### Patch Changes

- `peerDependencies` ranges should be compared loosely [#3753](https://github.com/pnpm/pnpm/issues/3753).
- Don't fail if a linked directory is not found. Just print a warning about it [#3746](https://github.com/pnpm/pnpm/issues/3746).

## 6.21.1

### Patch Changes

- When the store location is a relative location, pick the store location relative to the workspace root directory location [#3976](https://github.com/pnpm/pnpm/issues/3976).
- Don't crash if a bin file cannot be created because the source files could not be found.
- Use the system default Node.js version to check package compatibility [#3785](https://github.com/pnpm/pnpm/issues/3785).

## 6.21.0

### Minor Changes

- Support async hooks [#3955](https://github.com/pnpm/pnpm/pull/3955).
- Added support for a new lifecycle script: `pnpm:devPreinstall`. This script works only in the root `package.json` file, only during local development, and runs before installation happens [#3968](https://github.com/pnpm/pnpm/pull/3968).

### Patch Changes

- Installing a workspace project with an injected dependency from a non-root directory should not fail [#3970](https://github.com/pnpm/pnpm/issues/3970).
- Escape the arguments that are passed to the scripts [#3907](https://github.com/pnpm/pnpm/issues/3907).

## 6.20.4

### Patch Changes

- Do not index the project directory if it should not be hard linked to any other project [#3949](https://github.com/pnpm/pnpm/issues/3949).
- The CLI should not exit before all the output is printed [#3526](https://github.com/pnpm/pnpm/issues/3526).

## 6.20.3

### Patch Changes

- All the `dependenciesMeta` fields should be duplicated to the lockfile.

## 6.20.2

### Patch Changes

- `pnpm import` should be able to import a workspace lockfile [#3908](https://github.com/pnpm/pnpm/issues/3908).
- Don't run pre/post scripts by default with recursive run commands [#3903](https://github.com/pnpm/pnpm/issues/3903).
- `pnpm env use` should use the network/proxy settings to make HTTP requests [#3942](https://github.com/pnpm/pnpm/pull/3942).
- It should be possible to install a git-hosted package that has a port in its URL [#3944](https://github.com/pnpm/pnpm/issues/3944).
- `pnpm create` and `pnpm dlx` should work with scoped packages [#3916](https://github.com/pnpm/pnpm/issues/3916).

## 6.20.1

### Patch Changes

- Fix broken artifacts of `@pnpm/exe`. This doesn't affect the `pnpm` package.

  Related issue: [#3937](https://github.com/pnpm/pnpm/issues/3937). This was a bug introduced by [#3896](https://github.com/pnpm/pnpm/pull/3896).

## 6.20.0

### Minor Changes

- New property supported via the `dependenciesMeta` field of `package.json`: `injected`. When `injected` is set to `true`, the package will be hard linked to `node_modules`, not symlinked [#3915](https://github.com/pnpm/pnpm/pull/3915).

  For instance, the following `package.json` in a workspace will create a symlink to `bar` in the `node_modules` directory of `foo`:

  ```json
  {
    "name": "foo",
    "dependencies": {
      "bar": "workspace:1.0.0"
    }
  }
  ```

  But what if `bar` has `react` in its peer dependencies? If all projects in the monorepo use the same version of `react`, then no problem. But what if `bar` is required by `foo` that uses `react` 16 and `qar` with `react` 17? In the past, you'd have to choose a single version of react and install it as dev dependency of `bar`. But now with the `injected` field you can inject `bar` to a package, and `bar` will be installed with the `react` version of that package.

  So this will be the `package.json` of `foo`:

  ```json
  {
    "name": "foo",
    "dependencies": {
      "bar": "workspace:1.0.0",
      "react": "16"
    },
    "dependenciesMeta": {
      "bar": {
        "injected": true
      }
    }
  }
  ```

  `bar` will be hard linked into the dependencies of `foo`, and `react` 16 will be linked to the dependencies of `foo/node_modules/bar`.

  And this will be the `package.json` of `qar`:

  ```json
  {
    "name": "qar",
    "dependencies": {
      "bar": "workspace:1.0.0",
      "react": "17"
    },
    "dependenciesMeta": {
      "bar": {
        "injected": true
      }
    }
  }
  ```

  `bar` will be hard linked into the dependencies of `qar`, and `react` 17 will be linked to the dependencies of `qar/node_modules/bar`.

### Patch Changes

- Buffer warnings fixed [#3932](https://github.com/pnpm/pnpm/issues/3932).

## 6.19.1

### Patch Changes

- Proxy URLs with special characters in credentials should work [#3925](https://github.com/pnpm/pnpm/pull/3925).
- Don't print info message about conflicting command names [#3912](https://github.com/pnpm/pnpm/pull/3912).

## 6.19.0

### Minor Changes

- Package scope is optional when filtering by package name [#3485](https://github.com/pnpm/pnpm/pull/3458).

  So the next two commands will both find `@pnpm/core`:

  ```
  pnpm test --filter core
  pnpm test --filter @pnpm/core
  ```

  However, if the workspace contains `@types/core` and `@pnpm/core`, `--filter=core` will not work.

- Allow a system's package manager to override pnpm's default settings

### Patch Changes

- `pnpm install --global` should link global packages to specific Node.js versions only if Node.js was installed by pnpm [#3910](https://github.com/pnpm/pnpm/pull/3910).
- It should be possible to alias a workspace package that has a name with a scope [#3899](https://github.com/pnpm/pnpm/pull/3899).
- `pnpm store path` added to the output of `pnpm store`.

## 6.18.0

### Minor Changes

- `pnpm env use`:
  - allow to install the latest Node.js release [#3879](https://github.com/pnpm/pnpm/pull/3879):
    ```
    pnpm env use -g latest
    ```
  - allow to install prerelease versions of Node.js [#3892](https://github.com/pnpm/pnpm/pull/3892):
    ```
    pnpm env use -g 16.0.0-rc.0
    pnpm env use -g nightly
    pnpm env use -g nightly/16
    ```
- `maxsockets`: a new setting to configure the maximum number of connections to use per origin (protocol/host/post combination) [#3889](https://github.com/pnpm/pnpm/pull/3889).

### Patch Changes

- Installing Git-hosted dependencies should work using URLs with colon. For instance, `pnpm add ssh://git@example.com:foo/bar.git` [#3882](https://github.com/pnpm/pnpm/pull/3882).
- Autofix command files with Windows line endings on the shebang line [#3887](https://github.com/pnpm/pnpm/pull/3887).

## 6.17.2

### Patch Changes

- Dedupe dependencies when adding new ones or updating existing ones [#2222](https://github.com/pnpm/pnpm/issues/2222).
- Only check for CLI updates when `pnpm install` or `pnpm add` is executed [#3874](https://github.com/pnpm/pnpm/pull/3874).
- Use a single global config file (at `~/.config/pnpm/npmrc`) for all npm versions, when npm is installed via `pnpm env use` [#3873](https://github.com/pnpm/pnpm/pull/3873).
- Add information about the `--force` option into `pnpm install --help` [#3878](https://github.com/pnpm/pnpm/pull/3878).
- Allow to pass the `--cache-dir` and `--save-prefix` CLI options.

## 6.17.1

### Patch Changes

- `pnpm env use` should create a symlink to the Node.js executable, not a command shim [#3869](https://github.com/pnpm/pnpm/pull/3869).
- Attach the globally installed packages to the system default Node.js executable [#3870](https://github.com/pnpm/pnpm/pull/3870).
- The `.pnpm-debug.log` file is not written when pnpm CLI exits with an expected non-zero exit code. For instance, when vulnerabilities are found by the `pnpm audit` command [#3832](https://github.com/pnpm/pnpm/issues/3832).
- Suggest `pnpm install --force` to refetch modified packages [#3867](https://github.com/pnpm/pnpm/pull/3867).

## 6.17.0

### Minor Changes

- New hook supported for filtering out info and warning logs: `filterLog(log) => boolean` [#3802](https://github.com/pnpm/pnpm/pull/3802).
- New command added: `pnpm create` is similar to `yarn create` [#3829](https://github.com/pnpm/pnpm/pull/3829).
- `pnpm dlx` supports the `--silent` option [#3839](https://github.com/pnpm/pnpm/pull/3839).

### Patch Changes

- Add link to the docs to the help output of the dlx, exec, root, and bin commands [#3838](https://github.com/pnpm/pnpm/pull/3838).
- Don't print anything except the JSON output, when the `--json` option is used [#3844](https://github.com/pnpm/pnpm/pull/3844).

## 6.16.1

### Patch Changes

- Installation should not fail if the installed package has no `package.json` [#3782](https://github.com/pnpm/pnpm/pull/3782).
- Hoisting should work when the dependencies of only a subset of workspace projects are installed [#3806](https://github.com/pnpm/pnpm/pull/3806).
- Upgraded ansi-regex to v5.0.1 in order to fix a security vulnerability [CVE-2021-3807](https://github.com/advisories/GHSA-93q8-gq69-wqmw).

## 6.16.0

### Minor Changes

- New setting added: `changed-files-ignore-pattern`. It allows to ignore changed files by glob patterns when filtering for changed projects since the specified commit/branch [#3797](https://github.com/pnpm/pnpm/pull/3797).
- New setting added: `extend-node-path`. When it is set to `false`, pnpm does not set the `NODE_PATH` environment variable in the command shims [#3799](https://github.com/pnpm/pnpm/pull/3799).

### Patch Changes

- Pick the right extension for command files. It is important to write files with .CMD extension on case sensitive Windows drives [#3804](https://github.com/pnpm/pnpm/pull/3804).

## 6.15.2

### Patch Changes

- `pnpm add --global <pkg>` should use an exact path to the Node.js executable to create the command shim. This way, the globally install package will work even if the system-wide Node.js is switched to another version [#3780](https://github.com/pnpm/pnpm/pull/3780).
- `pnpm install --fix-lockfile` should not ignore the `dependencies` field in the existing lockfile [#3774](https://github.com/pnpm/pnpm/pull/3774).
- When `use-beta-cli` is `true`, the global packages directory is inside the pnpm home directory [#3781](https://github.com/pnpm/pnpm/pull/3781).
- `pnpm install --frozen-lockfile` should not fail if a project has a local directory dependency that has no manifest (`package.json` file) [#3793](https://github.com/pnpm/pnpm/pull/3793).
- Don't override the bin files of direct dependencies with the bin files of hoisted dependencies [#3795](https://github.com/pnpm/pnpm/pull/3795).

## 6.15.1

### Patch Changes

- A security vulnerability fixed. When commands are executed on Windows, they should not be searched for in the current working directory.
- `pnpm import` should never run scripts [#3750](https://github.com/pnpm/pnpm/issues/3750).

## 6.15.0

### Minor Changes

- `pnpm install --fix-lockfile` allows to fix a broken lockfile [#3729](https://github.com/pnpm/pnpm/pull/3729).
- New setting supported: `global-bin-dir`. `global-bin-dir` allows to set the target directory for the bin files of globally installed packages [#3762](https://github.com/pnpm/pnpm/pull/3762).

### Patch Changes

- The pnpm CLI should not silently exit on bad HTTPS requests [#3768](https://github.com/pnpm/pnpm/pull/3768).

## 6.14.7

### Patch Changes

- Use correct GitLab tarball URL [#3643](https://github.com/pnpm/pnpm/issues/3643).
- Accept gzip and deflate encoding from the registry [#3745](https://github.com/pnpm/pnpm/pull/3745).
- Print error codes in error messages [#3748](https://github.com/pnpm/pnpm/pull/3748).
- Allow the \$ sign to be a command name [#3679](https://github.com/pnpm/pnpm/issues/3679).

## 6.14.6

### Patch Changes

- `pnpm setup` should add pnpm to the PATH on Windows [#3734](https://github.com/pnpm/pnpm/pull/3734).
- `pnpm env` should not create PowerShell command shims to fix issues on Windows [#3711](https://github.com/pnpm/pnpm/issues/3711).
- `overrides` should work with selectors that specify the parent package with a version range [#3732](https://github.com/pnpm/pnpm/issues/3732).

## 6.14.5

### Patch Changes

- A broken `package.json` should not make pnpm exit without any message [#3705](https://github.com/pnpm/pnpm/issues/3705).
- `pnpm dlx` should allow to pass multiple packages for installation [#3710](https://github.com/pnpm/pnpm/pull/3710).
- The pnpm home directory should be always preferred when searching for a global bin directory [#3723](https://github.com/pnpm/pnpm/pull/3723).
- `pnpm setup` should not remove the pnpm CLI executable, just copy it to the pnpm home directory [#3724](https://github.com/pnpm/pnpm/pull/3724).
- It should be possible to set `cache-dir` and `state-dir` through config files [#3727](https://github.com/pnpm/pnpm/pull/3727).

## 6.14.3

### Patch Changes

- Downgrade `@yarnpkg/parsers` to v2.3.0 from v2.4.0 to fix a regression in script running, when `shell-emulator` is `true` [#3700](https://github.com/pnpm/pnpm/issues/3700).

## 6.14.2

### Patch Changes

- `pnpm setup` prints an info message that suggests to open a new terminal [#3698](https://github.com/pnpm/pnpm/pull/3698).
- `pnpm env use -g <version>` links `npm` as well, when installing Node.js [#3696](https://github.com/pnpm/pnpm/pull/3696).

## 6.14.1

### Patch Changes

- Don't crash on unsupported packages that are only dependencies of skipped optional dependencies [#3640](https://github.com/pnpm/pnpm/issues/3640).
- Allow to symlink a directory that has no `package.json` [#3691](https://github.com/pnpm/pnpm/issues/3691).

## 6.14.0

### Minor Changes

- `pnpm import` can convert a `yarn.lock` to a `pnpm-lock.yaml` [#3655](https://github.com/pnpm/pnpm/pull/3655).
- Backward-compatible change to the lockfile format. Optional dependencies will always have the `requiresBuild` field set to `true`. This change is needed to allow skipping optional dependency downloads, when the optional dependency is not compatible with the target system [#2038](https://github.com/pnpm/pnpm/issues/2038)

### Patch Changes

- Those optional dependencies that don't support the target system should not be downloaded from the registry [#2038](https://github.com/pnpm/pnpm/issues/2038).

## 6.13.0

### Minor Changes

- New command added for running packages in a temporary environment: `pnpm dlx <command> ...` [#3652](https://github.com/pnpm/pnpm/pull/3652).

### Patch Changes

- Link the package's own binaries before running its lifecycle scripts [#3662](https://github.com/pnpm/pnpm/pull/3662).
- Resolve peer dependencies from the root of the workspace when adding a new dependency or updating [#3667](https://github.com/pnpm/pnpm/pull/3667).
- Ignore empty shasum in entries in package metadata [#3666](https://github.com/pnpm/pnpm/pull/3666).
- Throw a meaningful error if a package has invalid shasum in its metadata [#3666](https://github.com/pnpm/pnpm/pull/3666).
- Add `run` to NO_SCRIPT error example [#3660](https://github.com/pnpm/pnpm/pull/3660).

## 6.12.1

### Patch Changes

- Fix a regression introduced in v6.12.0: `--workspace-root` optional should not be ignored.

## 6.12.0

### Minor Changes

- A new command added for installing Node.js: `pnpm env use --global <version>`.

  You may install Node.js using an exact version, version range, LTS, or LTS name (e.g. `argon`).

  Examples:

  ```
  pnpm env use --global 16.5.0
  pnpm env use --global 16
  pnpm env use --global lts
  pnpm env use --global argon
  ```

  Related PR: [#3620](https://github.com/pnpm/pnpm/pull/3620).

- Exclude the root package, when running `pnpm exec|run|add`. This change is only active when `use-beta-cli` is set to `true` [#3647](https://github.com/pnpm/pnpm/pull/3647).

- When `--workspace-root` is used, the workspace root package is selected even if the command is executed with filters [#3647](https://github.com/pnpm/pnpm/pull/3647).

  For example: `pnpm --workspace-root --filter=foo run lint`

### Patch Changes

- Avoid too many open files error [#3637](https://github.com/pnpm/pnpm/pull/3637).
- `pnpm audit --fix` should not add overrides for packages with vulnerabilities that do not have fixes [#3649](https://github.com/pnpm/pnpm/pull/3649).
- When a peer dependency issue happens, the warning should contain the path to the project with the issue [#3650](https://github.com/pnpm/pnpm/pull/3650).

## 6.11.5

### Minor Changes

- New `package.json` setting added: `publishConfig.executableFiles`. By default, for portability reasons, no files except those listed in the bin field will be marked as executable in the resulting package archive. The `executableFiles` field lets you declare additional fields that must have the executable flag (+x) set even if they aren't directly accessible through the bin field.

  ```json
  {
    ...
    "publishConfig": {
      "executableFiles": [
        "./dist/shim.js",
      ]
    }
  }
  ```

## 6.11.2

### Patch Changes

- Fix a regression introduced in v6.11.0 [#3627](https://github.com/pnpm/pnpm/pull/3627).

## 6.11.1

### Patch Changes

- Print a warning when a lifecycle script is skipped [#3619](https://github.com/pnpm/pnpm/pull/3619).

## 6.11.0

### Minor Changes

- New command added: `pnpm audit --fix`. This command adds overrides to `package.json` that force versions of packages that do not have the vulnerabilities [#3598](https://github.com/pnpm/pnpm/pull/3598).
- Own implementation of `pnpm pack` is added. It is not passed through to `npm pack` anymore [#3608](https://github.com/pnpm/pnpm/pull/3608).
- When `pnpm add pkg` is executed in a workspace and `pkg` is already in the dependencies of one of the workspace projects, pnpm uses that already present version range to add the new dependency [#3614](https://github.com/pnpm/pnpm/pull/3614).

### Patch Changes

- Don't collapse warnings when `--reporter append-only` is used.

## 6.10.3

### Patch Changes

- Overriding packages by parent package and no range. Fixes a regression introduced in v6.10.2

## 6.10.2

### Patch Changes

- `pnpm exec` should work outside of Node.js projects [#3597](https://github.com/pnpm/pnpm/pull/3597).
- Overriding should work when the range selector contains `>`.

## 6.10.1

### Patch Changes

- A trailing newline should always be added to the output [#3565](https://github.com/pnpm/pnpm/pull/3565).
- `pnpm link --global` should not break global dependencies [#3462](https://github.com/pnpm/pnpm/issues/3462).

## 6.10.0

### Minor Changes

- New command added: `pnpm store path` [#3571](https://github.com/pnpm/pnpm/pull/3571).
- New setting added: `cache-dir`. `cache-dir` is the location of the package metadata cache. Previously this cache was stored in the store directory. By default, the cache directory is created in the `XDG_CACHE_HOME` directory [#3578](https://github.com/pnpm/pnpm/pull/3578).
- New setting added: `state-dir`. `state-dir` is the directory where pnpm creates the `pnpm-state.json` file that is currently used only by the update checker. By default, the state directory is created in the `XDG_STATE_HOME` directory [#3580](https://github.com/pnpm/pnpm/pull/3580).
- `workspace-concurrency` is based on CPU cores amount, when set to 0 or a negative number. The concurrency limit is set as `max((amount of cores) - abs(workspace-concurrency), 1)` [#3574](https://github.com/pnpm/pnpm/pull/3574).

### Patch Changes

- Never do full resolution when package manifest is ignored [#3576](https://github.com/pnpm/pnpm/issues/3576).
- An error is thrown if `pnpm link` is executed. `pnpm link` needs at least one argument or option [#3590](https://github.com/pnpm/pnpm/pull/3590).

## 6.9.1

### Patch Changes

- Dependencies from the root workspace package should be used to resolve peer dependencies of any projects in the workspace.

## 6.9.0

### Minor Changes

- A new optional field added to the `pnpm` section of `package.json`: `packageExtensions`. The `packageExtensions` fields offer a way to extend the existing package definitions with additional information. For example, if `react-redux` should have `react-dom` in its `peerDependencies` but it has not, it is possible to patch `react-redux` using `packageExtensions`:

  ```json
  {
    "pnpm": {
      "packageExtensions": {
        "react-redux": {
          "peerDependencies": {
            "react-dom": "*"
          }
        }
      }
    }
  }
  ```

  The keys in `packageExtensions` are package names or package names and semver ranges, to it is possible to patch only some versions of a package:

  ```json
  {
    "pnpm": {
      "packageExtensions": {
        "react-redux@1": {
          "peerDependencies": {
            "react-dom": "*"
          }
        }
      }
    }
  }
  ```

  The following fields may be extended using `packageExtensions`: `dependencies`, `optionalDependencies`, `peerDependencies`, and `peerDependenciesMeta`.

  A bigger example:

  ```json
  {
    "pnpm": {
      "packageExtensions": {
        "express@1": {
          "optionalDependencies": {
            "typescript": "2"
          }
        },
        "fork-ts-checker-webpack-plugin": {
          "dependencies": {
            "@babel/core": "1"
          },
          "peerDependencies": {
            "eslint": ">= 6"
          },
          "peerDependenciesMeta": {
            "eslint": {
              "optional": true
            }
          }
        }
      }
    }
  }
  ```

## 6.8.0

### Minor Changes

- When `use-beta-cli` is `true`, filtering by directories supports globs [#3521](https://github.com/pnpm/pnpm/pull/3521).

### Patch Changes

- The `pnpm remove` and `pnpm update` commands do not fail when the `dev`, `production`, or `optional` settings are set.
- Use the real package names of the peer dependencies when creating the paths in the virtual store [#3545](https://github.com/pnpm/pnpm/pull/3545).
- The lockfile should not break on peer dependencies that have underscores in their name [#3546](https://github.com/pnpm/pnpm/pull/3546).
- Resolve peer dependencies from the dependencies of the root project of the workspace [#3549](https://github.com/pnpm/pnpm/pull/3549).

## 6.7.6

### Patch Changes

- Generate a correct command shim on Windows if pnpm is installed to a directory with spaces in its name [#3519](https://github.com/pnpm/pnpm/issues/3519).

## 6.7.4

### Patch Changes

- `pnpm exec` should run the command in the right directory, when executed inside a workspace [#3514](https://github.com/pnpm/pnpm/pull/3514).

## 6.7.3

### Patch Changes

- When publishing in a workspace, pass the `.npmrc` file from the root of the workspace to npm [#3511](https://github.com/pnpm/pnpm/pull/3511).

## 6.7.2

### Patch Changes

- It should be possible to install a Git-hosted package that uses a default branch other than "master" [#3506](https://github.com/pnpm/pnpm/pull/3506).
- It should be possible to install a Git-hosted package by using only part of the Git commit hash [#3507](https://github.com/pnpm/pnpm/pull/3507).

## 6.7.1

### Minor Changes

- Support the `publishConfig.directory` field in `package.json` [#3490](https://github.com/pnpm/pnpm/pull/3490).
- There is no need to escape the command shell with `--`, when using the exec command. So just `pnpm exec rm -rf dir` instead of `pnpm exec -- rm -rf dir` [#3492](https://github.com/pnpm/pnpm/pull/3492).
- `pnpm audit` supports a new option: `--ignore-registry-errors`. `pnpm audit --ignore-registry-errors` exits with exit code 0, when the registry responds with a non-200 status code [#3472](https://github.com/pnpm/pnpm/pull/3472).

### Patch Changes

- Mention `pnpm exec` in the generic help.
- `pnpm outdated` should read the value of the `strict-ssl` setting [#3493](https://github.com/pnpm/pnpm/issues/3493).
- New lines in engine field should not break the lockfile [#3491](https://github.com/pnpm/pnpm/issues/3491).

## 6.6.2

### Patch Changes

- a1a03d145: Import only the required functions from ramda.

## 6.6.1

### Minor Changes

- When pnpm is executed with an unknown command, it is considered a shell command that needs to be executed in the context of the project. So you can do things like `pnpm eslint`, when eslint is in the dependencies. It is kind of similar to `pnpx eslint` but unlink `pnpx`, `pnpm eslint` will not install eslint, when not present [#3478](https://github.com/pnpm/pnpm/pull/3478).

## 6.5.0

### Minor Changes

- New setting added: `use-node-version`. When set, pnpm will install the specified version of Node.js and use it for running any lifecycle scripts [#3459](https://github.com/pnpm/pnpm/pull/3459).
- New experimental command added: `pnpm setup`. This command adds the path to the pnpm bin to the active shell of the user. So it modifies the bash, zsh, or fish config file [#3456](https://github.com/pnpm/pnpm/pull/3456).
- `pnpm publish -r` supports a new option: `--report-summary`. When it is used, `pnpm publish -r --report-summary` will save the summary of published packages to `pnpm-publish-summary.json` [#3461](https://github.com/pnpm/pnpm/pull/3461).
- New CLI option added: `--use-stderr`. When set, all the output is written to stderr [#3463](https://github.com/pnpm/pnpm/pull/3463).
- pnpm now reads the value of the `NPM_CONFIG_WORKSPACE_DIR` env variable to find the directory that contains the workspace manifest file. By default pnpm will look in all parent directories for this file [#3464](https://github.com/pnpm/pnpm/pull/3464).

### Patch Changes

- Do not retry requests, when checking for new versions of pnpm [#3465](https://github.com/pnpm/pnpm/pull/3465).

## 6.4.0

### Minor Changes

- Added support for `type` and `imports` in `publishConfig` field of the `package.json` manifest [#3315](https://github.com/pnpm/pnpm/pull/3315).

### Patch Changes

- Do not print a warning if a skipped optional dependency cannot be hoisted [#3454](https://github.com/pnpm/pnpm/pull/3454).
- The second argument to readPackage hook should always be the context object [#3455](https://github.com/pnpm/pnpm/pull/3455).

## 6.3.0

### Minor Changes

- `pnpm list -r --json` returns the locations of workspace projects via the `path` field [#3432](https://github.com/pnpm/pnpm/pull/3432/files).

### Patch Changes

- `save-prefix` should be respected, when it is set to empty [#3414](https://github.com/pnpm/pnpm/issues/3414).
- skip resolution, when lockfile is up-to-date, even if some packages in the workspace are referenced through relative path [#3422](https://github.com/pnpm/pnpm/pull/3422).
- `pnpm why`: do not incorrectly include linked deps in search results [#3428](https://github.com/pnpm/pnpm/pull/3428).

## 6.2.5

### Patch Changes

- Do not crash when linking two dependencies with the same name [#3308](https://github.com/pnpm/pnpm/issues/3308).
- The temp pnpx directory should be created inside the pnpm store.

## 6.2.4

### Patch Changes

- pnpm should not fail with an `EMFILE` error on a big workspace with many projects [#3381](https://github.com/pnpm/pnpm/pull/3381).

## 6.2.3

### Patch Changes

- Fixing a regression introduced in v6.2.2 [#3407](https://github.com/pnpm/pnpm/issues/3407).
- The `child-concurrency` setting should not be ignored when installing in a project with up-to-date lockfile [#3399](https://github.com/pnpm/pnpm/issues/3399).

## 6.2.2

### Patch Changes

- `pnpm audit` should not receive a 502 error from the registry [#2848](https://github.com/pnpm/pnpm/issues/2848).
- When installing Git-hosted dependencies that have a `prepare` script, pnpm should install their `devDependencies` for a successfully build [#855](https://github.com/pnpm/pnpm/issues/855).
- `preinstall` scripts should run after installing the dependencies [#3395](https://github.com/pnpm/pnpm/pull/3395).
- Sorting workspace projects should work correctly when the workspace dependencies use `workspace:~` or `workspace:^` [#3400](https://github.com/pnpm/pnpm/issues/3400)

## 6.2.1

### Minor Changes

- New CLI option: `--filter-prod`. `--filter-prod` acts the same as `--filter`, but it omits `devDependencies` when selecting dependency projects from the workspace [#3372](https://github.com/pnpm/pnpm/pull/3372).
- New types of workspace ranges added [#3116](https://github.com/pnpm/pnpm/issues/3116):
  - `workspace:~` means that the version of the workspace project should be added using the `~` prefix. For instance: `~1.0.0` (if the version of the referenced project is `1.0.0` in the workspace).
  - `workspace:^` means that the version of the workspace project should be added using the `^` prefix. For instance: `^1.0.0`.
- New setting: `fetch-timeout`. Sets the maximum amount of time to wait for HTTP requests to complete. By default, the value is 60000 (1 minute) [#3390](https://github.com/pnpm/pnpm/pull/3390).

### Patch Changes

- Don't skip lifecycle scripts of projects when doing a filtered installation [#3251](https://github.com/pnpm/pnpm/issues/3251).
- No deprecation warning about `rmdir()` usage should appear when running pnpm on Node.js 16.
- Link overrides should work on non-root workspace projects [#3388](https://github.com/pnpm/pnpm/pull/3388).
- pnpm should not fail with an `EMFILE` error on a big workspace with many projects [#3381](https://github.com/pnpm/pnpm/pull/3381).

## 6.1.0

### Minor Changes

- New option added: `enable-pre-post-scripts`. When it is set to `true`, lifecycle scripts with pre/post prefixes are automatically executed by pnpm [#3348](https://github.com/pnpm/pnpm/pull/3348).

## 6.0.2

### Bug Fixes

- `pnpm publish`: lifecycle scripts should not be executed twice when the globally installed npm version is 7 [#3340](https://github.com/pnpm/pnpm/pull/3340).
- `pnpm list`: hoisted dependencies are not listed as unsaved dependencies [#3339](https://github.com/pnpm/pnpm/pull/3339).
- `pnpm.overrides` should override direct dev dependencies [#3327](https://github.com/pnpm/pnpm/pull/3327).
- Commands from the root of the workspace should be in the PATH even when there is no lockfile in the workspace root [#2086](https://github.com/pnpm/pnpm/issues/2086).

## 6.0.1

### Bug Fixes

- Use `+` instead of `#` in directory names inside the virtual store directory (`node_modules/.pnpm`). `#` causes issues with Webpack and Vite [#3314](https://github.com/pnpm/pnpm/pull/3314).

## 6.0.0

### Major Changes

- Node.js v10 support is dropped. At least Node.js v12.17 is required for the package to work.

- Arbitrary pre/post hooks for user-defined scripts (such as `prestart`) are not executed automatically.

- The lockfile version is bumped to v5.3. Changes in the new format:

  - Blank lines added between package/project entries to improve readability and decrease merge issues.
  - The `resolution`, `engines`, `os`, and `cpu` fields are now always written in a single lines, as the first keys of the package objects.
  - A new field is added to the package objects: `transitivePeerDependencies`.

- The layout of the virtual store directory has changed (`node_modules/.pnpm`) to allow keeping cache in it:

  - All packages inside the virtual store directory are on the same depth. Instead of subdirectories, one directory is used with `#` instead of slashes.
  - New setting added: `modules-cache-max-age`. The default value of the setting is 10080 (7 days in minutes). `modules-cache-max-age` is the time in minutes after which pnpm should remove the orphan packages from `node_modules`.

- pnpx does not automatically install packages. A prompt asks the user if a package should be installed, if it is not present.

  `pnpx --yes` tells pnpx to install any missing package.

  `pnpx --no` makes pnpx fail if the called packages is not installed.

- `pnpmfile.js` renamed to `.pnpmfile.cjs` in order to force CommonJS.

- `.pnp.js` renamed to `.pnp.cjs` in order to force CommonJS.

- The `pnpm-prefix` setting is removed. Use `global-dir` to specify a custom location for the globally installed packages.

- The default depth of an update is `Infinity`, not `0`.

- The `--global` option should be used when linking from/to the global modules directory.

  Linking a package to the global directory:

  - pnpm v5: `pnpm link`
  - pnpm v6: `pnpm link --global`

  Linking a package from the global directory:

  - pnpm v5: `pnpm link foo`
  - pnpm v6: `pnpm link --global foo`

- pnpm's command file's extension changed to `.cjs` (`bin/pnpm.js`=>`bin/pnpm.cjs`).

- [node-gyp](https://github.com/nodejs/node-gyp) updated to v8.

- `prepublish` is not executed on a local `pnpm install`. Use `prepare` instead.

### Minor Changes

- A new command added: [pnpm fetch](https://pnpm.io/cli/fetch).

  Fetch packages from a lockfile into virtual store, package manifest is ignored.
  This command is specifically designed to boost building a docker image.

- Overrides match dependencies by checking if the target range is a subset of the specified range, instead of making an exact match.

  For example, the following override will replace any version of `foo` that has a subrange on v2:

  ```json
  "pnpm": {
    "overrides": {
      "foo@2": "2.1.0"
    }
  }
  ```

  This will override `foo@2.2.0` and `foo@^2.3.0` to `foo@2.1.0` as both `2.2.0` and `^2.3.0` are subranges of `2`.

## 5.18.9

### Patch Changes

- `pnpm store status` should look for the `integrity.json` file at the right place ([#2597](https://github.com/pnpm/pnpm/issues/2597)).
- Allow `--https-proxy`, `--proxy`, and `--noproxy` CLI options with the `install`, `add`, `update` commands ([#3274](https://github.com/pnpm/pnpm/issues/3274)).

## 5.18.8

### Patch Changes

- Installation of packages that have bin directories with subdirectories should not fail ([#3263](https://github.com/pnpm/pnpm/issues/3263)).
- The value of the `noproxy` setting should be read ([#3258](https://github.com/pnpm/pnpm/issues/3258)).
- An empty `node_modules` directory should not be created just to save a `.pnpm-debug.log` file to it.

## 5.18.7

### Patch Changes

- Proxying through `socks://` should work ([#3241](https://github.com/pnpm/pnpm/issues/3241)).
- Non-directories should not be added to `NODE_PATH` in command shims ([#3156](https://github.com/pnpm/pnpm/issues/3156)).

## 5.18.6

### Patch Changes

- Escape invalid characters in file names, when linking packages from the store ([#3232](https://github.com/pnpm/pnpm/pull/3232)).
- Link to the compatibility page fixed.

## 5.18.5

### Patch Changes

- Verify the name and version of the package before linking it from the store ([PR #3224](https://github.com/pnpm/pnpm/pull/3224), [issue #3188](https://github.com/pnpm/pnpm/issues/3188)).
- The lockfile should be autofixed if it contains broken integrity checksums ([PR #3228](https://github.com/pnpm/pnpm/pull/3228), [issue #3137](https://github.com/pnpm/pnpm/issues/3137)).

## 5.18.4

### Patch Changes

- Links to docs updated. The docs now lead to the versioned `5.x` docs, not the current ones.
- The command prompts should work when selecting shell target in `pnpm install-completion` [#3221](https://github.com/pnpm/pnpm/issues/3221).

## 5.18.3

### Patch Changes

- Broken lockfiles are ignore unless `pnpm install --frozen-lockfile` is used [#1395](https://github.com/pnpm/pnpm/issues/1395).
- Fixed occasional "Too many open file" error on `pnpm store status` [#3185](https://github.com/pnpm/pnpm/issues/3185).

## 5.18.2

### Patch Changes

- `pnpm audit` should work with the `--no-optional`, `--dev`, and `--prod` options [#3152](https://github.com/pnpm/pnpm/issues/3152).

## 5.18.1

### Patch Changes

- The ID of a tarball dependency should not contain colons, when the URL has a port. The colon should be escaped with a plus sign[#3182](https://github.com/pnpm/pnpm/issues/3182).

## 5.18.0

### Minor Changes

- `pnpm publish -r --force` should try to publish packages even if their current version is already in the registry.

## 5.17.3

### Patch Changes

- Turn off warnings about settings.
- The `-P/-D` shorthand options should work with the `pnpm why` command.
- `pnpm add --global pnpm` does not create PowerShell command shims for the pnpm CLI.

## 5.17.2

### Patch Changes

- Only display unknown settings warning, when `pnpm install` is executed [#3130](https://github.com/pnpm/pnpm/pull/3130).
- Update help for the filter option. Some of the filtering patterns should be escaped in Zsh.
- Audit output should always have a new line at the end [#3134](https://github.com/pnpm/pnpm/pull/3134).
- Return the correct registry for an aliased scoped dependency [#3103](https://github.com/pnpm/pnpm/pull/3134).

## 5.17.1

### Minor Changes

- New '--reverse' CLI option added for reversing the order of package executions during `pnpm -r run` [#2985](https://github.com/pnpm/pnpm/issues/2985).

## 5.16.1

### Patch Changes

- Remove redundant empty lines when run `pnpm why --parseable` [#3101](https://github.com/pnpm/pnpm/pull/3101).
- `pnpm publish --publish-branch=<branch>` does not fail [#2996](https://github.com/pnpm/pnpm/issues/2996).
- Don't print warnings when `.npmrc` contains empty lines with whitespaces [#3105](https://github.com/pnpm/pnpm/pull/3105).

## 5.16.0

### Minor Changes

- Allow to ignore the builds of specific dependencies [#3080](https://github.com/pnpm/pnpm/pull/3080).

  The list of dependencies that should never be built, is specified through the `pnpm.neverBuiltDependencies` of `package.json`. For instance:

  ```json
  {
    "pnpm": {
      "neverBuiltDependencies": ["fsevents", "level"]
    }
  }
  ```

- Print warnings if unknown settings are found in `.npmrc` [#3074](https://github.com/pnpm/pnpm/pull/3074).

- pnpm can now be executed using its single bundled CLI file [#3096](https://github.com/pnpm/pnpm/pull/3096).

- When pnpm crashes because the Node.js version is unsupported, the error message will now contain a link to the compatibility page of the pnpm documentation website.

- `pnpm pubish -r` prints an info message if there are no pending packages to be published.

## 5.15.3

### Patch Changes

- A failing optional dependency should not cause a crash of headless installation [#3090](https://github.com/pnpm/pnpm/issues/3090).
- `npx pnpm install --global pnpm` should not install pnpm to the temporary directory of npx [#2873](https://github.com/pnpm/pnpm/issues/2873).

## 5.15.2

### Patch Changes

- The lockfile's content should not "flicker" if some dependency's version is specified with build metadata [#2928](https://github.com/pnpm/pnpm/issues/2928).
- If `pnpm.overrides` were modified, the resolution stage may never be skipped [#3079](https://github.com/pnpm/pnpm/pull/3079).
- The `pnpm` section of options is not included in the published version of a `package.json` [#3081](https://github.com/pnpm/pnpm/pull/3081).
- Fix the error message that happens when trying to add a new dependency to the root of a workspace [#3082](https://github.com/pnpm/pnpm/pull/3082).

## 5.15.1

### Patch Changes

- Finding the global directory location should not fail when one of the possible locations is in a read-only filesystem [#2794](https://github.com/pnpm/pnpm/issues/2794).

- Don't ask for confirmation, when publishing happens on a branch name `"main"` [#2995](https://github.com/pnpm/pnpm/issues/2995).

- Highlight the project names in the output of the `pnpm list` command [#3024](https://github.com/pnpm/pnpm/issues/3024).

- It should be possible to use the workspace protocol with version specs inside `pnpm.overrides` [#3029](https://github.com/pnpm/pnpm/issues/3029).

  For instance:

  ```json
  {
    "pnpm": {
      "overrides": {
        "foo": "workspace:*"
      }
    }
  }
  ```

## 5.15.0

### Minor Changes

- Allow to specify the shell target when configuring autocompletion with `pnpm install-completion`. For instance: `pnpm install-completion zsh`.
- New option added: `enable-modules-dir`. When `false`, pnpm will not write any files to the modules directory (node_modules). This is useful for when the modules directory is mounted with filesystem in userspace (FUSE). There is an experimental CLI that allows to mount a modules directory with FUSE: [@pnpm/mount-modules](https://www.npmjs.com/package/@pnpm/mount-modules).

### Patch Changes

- Fixed a performance regression that was caused by [#3032](https://github.com/pnpm/pnpm/pull/3032) and shipped in pnpm v5.13.7

  The performance of repeat `pnpm install` execution was in some cases significantly slower.

- Don't create broken symlinks to skipped optional dependencies, when hoisting. This issue was already fixed in pnpm v5.13.7 for the case when the lockfile is up-to-date. This fixes the same issue for cases when the lockfile needs updates. For instance, when adding a new package.

## 5.14.3

### Patch Changes

- Fixed an issue with installing peer dependencies. In some rare cases, when installing a new peer dependency, the other existing dependencies were removed [#3057](https://github.com/pnpm/pnpm/pull/3057)

## 5.14.2

### Patch Changes

- Linking dependencies by absolute path should work. For instance: `pnpm link C:\src\foo` [#3025](https://github.com/pnpm/pnpm/issues/3025)

## 5.14.1

### Minor Changes

- New option added: `test-pattern`. `test-pattern` allows to detect whether the modified files are related to tests. If they are, the dependent packages of such modified packages are not included.

  This option is useful with the "changed since" filter. For instance, the next command will run tests in all changed packages, and if the changes are in source code of the package, tests will run in the dependent packages as well:

  ```
  pnpm --filter=...[origin/master] --test-pattern=test/* test
  ```

- An exception is thrown if the workspace manifest is created with the wrong extension: `pnpm-workspace.yml` instead of `pnpm-workspace.yaml`.

### Patch Changes

- `--no-bail` should work with non-recursive commands [#3036](https://github.com/pnpm/pnpm/issues/3036).

## 5.13.7

### Patch Changes

- Broken symlinks are not created to skipped optional dependencies, when hoisting.

## 5.13.6

### Patch Changes

- Regression in `pnpm install-completion` fixed.
- Throw a meaningful error on malformed registry metadata.

## 5.13.5

### Patch Changes

- Include dependencies of dependents, when using `--filter ...pkg...` [#2917](https://github.com/pnpm/pnpm/issues/2917).
- Fix hanging requests issue. The number of max open sockets increased [#2998](https://github.com/pnpm/pnpm/pull/2998).

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

- When some of the dependencies of a package have the package as a peer dependency, don't make the dependency a peer dependency of itself.

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

- Excluding projects using `--filter=!<selector>` [#2804](https://github.com/pnpm/pnpm/issues/2804)

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

- The progress indicator also shows the number of dependencies that are being added to the modules directory (#2832).

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
- 160975d62: This fixes a regression introduced in pnpm v5.0.0. Direct local tarball dependencies should always be reanalyzed on install.

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
