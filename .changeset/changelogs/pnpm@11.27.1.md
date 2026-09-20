## 11.27.1

### Patch Changes

- `pn`, `pnpx`, and `pnx` now run the pnpm installed alongside them. They used to look pnpm up on `PATH`. That failed when the directory holding them was not on `PATH`, and it silently handed the call to an unrelated pnpm when one came first there [#14803](https://github.com/pnpm/pnpm/issues/14803).

- The `@zkochan/cmd-shim` package is now available as `@pnpm/bins.cmd-shim`.

- `pnpm cache list-registries` now prints the registry URL, matching `pnpm cache view`. It printed `https%3A+registry.npmjs.org` before and prints `https://registry.npmjs.org/` now [#15046](https://github.com/pnpm/pnpm/issues/15046).

- `pnpm deploy` no longer installs the dependencies of the workspace root project into the deploy directory [#6437](https://github.com/pnpm/pnpm/issues/6437).

- Install warnings no longer carry the text of a package's deprecation notice. The warning names the deprecated package and version, and the `pnpm:deprecation` event no longer carries the notice either. `pnpm view` still shows it on request.

  A deprecation warning now names the newest version of the package that is not deprecated, and says when reaching it means widening the range you declared:

  ```
  WARN  deprecated foo@1.0.0. 2.3.1 is not deprecated, outside the range you declared.
  ```

  pnpm works this out from the metadata it already fetched, so it costs no extra request. An install that reuses the lockfile without fetching metadata names no version.

  pnpm strips control characters from the package name and version in a deprecation warning, and from the notice `pnpm outdated --long` prints.

  The text sanitizer now also strips the Unicode line and paragraph separators U+2028 and U+2029.

- `pnpm exec <command>` and `pnpm <command>` run from a subdirectory of a project now find the executables installed in the project's `node_modules/.bin`. The command still runs in the subdirectory. `PNPM_PACKAGE_NAME` names the project [#5068](https://github.com/pnpm/pnpm/issues/5068).

- `pnpm exec` and `pnpm dlx` now wait for the command to finish shutting down after `Ctrl+C`. A signal sent to pnpm alone now reaches the command, the way it does with `pnpm run`. pnpm used to exit on the interrupt and terminate the command while it was still shutting down [#7374](https://github.com/pnpm/pnpm/issues/7374).

- Warnings about ignored environment variables in project `.npmrc` credentials now link to the npmrc documentation [pnpm/pnpm#15051](https://github.com/pnpm/pnpm/issues/15051).

- `pnpm audit --interactive --fix=update` no longer opens a second prompt for selecting dependencies to update [#14927](https://github.com/pnpm/pnpm/issues/14927).

- Fixed `pnpm dedupe` requiring a second pass after bumping a direct dependency in `package.json` [pnpm/pnpm#14987](https://github.com/pnpm/pnpm/issues/14987).

- `pnpm deploy` now writes plain versions for registry dependencies with peer dependencies in the deployed `package.json`. The deployed lockfile retains the resolved peer bindings. npm aliases keep their target package names [#14873](https://github.com/pnpm/pnpm/issues/14873).

- `pnpm publish` now allows a detached Git HEAD in CI, including checkouts of release tags. The working tree must still be clean. Branch and remote-history checks still apply when HEAD is attached [pnpm/pnpm#5894](https://github.com/pnpm/pnpm/issues/5894).

- `pnpm dlx` and `pnx` now prompt to approve dependency build scripts in interactive terminals. Cached packages with pending builds also prompt for approval. Without an interactive terminal, use `--allow-build` to allow the required builds. Fixes [pnpm/pnpm#14943](https://github.com/pnpm/pnpm/issues/14943).

- `pnpm install --force` now removes obsolete dependency links inside virtual-store packages when their dependencies change. Invalid dependency names are ignored during obsolete-link cleanup [#15039](https://github.com/pnpm/pnpm/issues/15039).

- `pnpm add -g` and `pnpm update -g` now ignore incomplete unrelated global package groups when every command from the replaced group is retained. Operations that could remove a global command still require complete ownership information.

- Resolving a Node.js runtime now fails when unofficial-builds.nodejs.org cannot be reached. pnpm used to ignore that failure and leave the musl builds out of `pnpm-lock.yaml`. `pnpm update` then wrote a different lockfile on a machine whose network blocks the mirror [pnpm/pnpm#14813](https://github.com/pnpm/pnpm/issues/14813).

- pnpm now deduplicates a package whose child dependency resolved an optional peer in one workspace project but not in another. Two copies of `next` could appear when only some projects could reach `styled-jsx`'s optional `babel-plugin-macros` peer [#14800](https://github.com/pnpm/pnpm/issues/14800).

- Fixed shell completion of package scripts for `pnpm run` and `pnpm run-script` [pnpm/pnpm#15034](https://github.com/pnpm/pnpm/issues/15034).

  Bash completion now preserves literal script names containing glob characters and shell punctuation in pnpm v11 and v12.

- `pnpm sbom` now publishes a valid URL in the CycloneDX `externalReferences[].url` and the SPDX `homepage`. An npm shorthand such as `vercel/ms` or `gitlab:group/subgroup/project` is expanded to the `git+https` URL npm derives for it. An scp-style remote such as `git@github.com:vercel/ms.git` is expanded the same way. Any other URL is published in its normalized form, without embedded credentials. A value that names no repository, an email address for example, is left out. pnpm used to publish the raw value, so a shorthand produced a URL that consumers such as Dependency-Track reject [pnpm/pnpm#14773](https://github.com/pnpm/pnpm/issues/14773).

- `pnpm setup` now describes the displayed configuration changes as "the following configuration changes."

- `pnpm --version` now reports why the pnpm version a project pins cannot be installed or recorded, then prints the version of the running CLI. It used to fail, which made the command unusable where the filesystem is read-only. `pnpm --version` also honors `--store-dir` and its `--store` alias now [#14831](https://github.com/pnpm/pnpm/issues/14831).

- `pnpm install --force` now reinstalls dependencies when the manifest and lockfile are unchanged. It previously reported "Already up to date" without reinstalling. Files changed in `node_modules` are restored when the store content is intact. Combining `--force` with `--frozen-store` now reports a configuration conflict on repeat installs [#919](https://github.com/pnpm/pnpm/issues/919).

- `pnpm install` now installs git-hosted dependencies without preparing them when their builds are explicitly denied by `allowBuilds`. Dependencies that require preparation still need an explicit allow or deny decision [pnpm/pnpm#10522](https://github.com/pnpm/pnpm/issues/10522).

- `pnpm runtime set` and `pnpm env use` now use the pnpm version that started the command. They could run a different installed pnpm when the command was started through Corepack or another wrapper.

- The install summary now names the version each dependency resolved to when `node-linker` is `hoisted`. It also lists what an install restores after `node_modules` is deleted, and both sides of a version change. The summary showed the range recorded in `package.json`, or nothing at all [#15161](https://github.com/pnpm/pnpm/issues/15161).

- The `@pnpm/npm-lifecycle` package is now available as `@pnpm/exec.npm-lifecycle`.

- Fixed `minimumReleaseAge` making pnpm download a package's full metadata again on every install. The cached copy carried a validator the registry could not match, so pnpm could never revalidate it [pnpm/pnpm#15103](https://github.com/pnpm/pnpm/issues/15103).

- pnpm now measures a `pnpm.overrides` entry written as a bare path, such as `./local-dep`, from the directory holding `pnpm-workspace.yaml`. It used to be measured from each package the override rewrote, so the dependency linked to a directory that does not exist [#11131](https://github.com/pnpm/pnpm/issues/11131).

- pnpm now preserves scalar YAML anchors and aliases when editing `pnpm-workspace.yaml`. Removing the entry that defines an anchor keeps surviving aliases valid. Entries updated to different values are written separately [#8245](https://github.com/pnpm/pnpm/issues/8245).

- pnpm now preserves comments and existing key order when updating `package.yaml`. New keys are appended to their mapping [pnpm/pnpm#2008](https://github.com/pnpm/pnpm/issues/2008).

- `pnpm install --prod` no longer downloads the registry packages that only a devDependency reaches [#881](https://github.com/pnpm/pnpm/issues/881).

- `pnpm update --global` no longer reinstalls a global package when its dependency graph resolves to what is already installed. It reports `Already up to date` [pnpm/pnpm#12002](https://github.com/pnpm/pnpm/issues/12002).

- The `minimumReleaseAge` approval prompt now counts and displays each package version once [pnpm/pnpm#15083](https://github.com/pnpm/pnpm/issues/15083).

- `pnpm run` no longer sends a script a second `SIGINT` when `Ctrl+C` is pressed in a terminal. A script that shuts down on the first `SIGINT` and exits at once on a second used to die before its shutdown finished [#7374](https://github.com/pnpm/pnpm/issues/7374).

- pnpm now reads a `pnpm-workspace.yaml` whose `tasks` section uses a setting only pnpm 12 acts on, such as `concurrencyGroup`. A task's unrecognized fields are ignored, unless the field only differs in case from `concurrency` or `dependsOn`, which pnpm reports as a typo.

  The warning about unrecognized top-level settings now names `cargo`, `concurrencyGroups`, and `pipelines` as pnpm 12 settings.

- `pnpm -r list --json` now prints one JSON array. It printed a separate array for each project when `sharedWorkspaceLockfile` was `false`, so the output could not be parsed.

  `pnpm -r list` now reads each project's own modules directory when the projects keep their own lockfiles, so `--long` and `--parseable` report the packages that project installed [#15011](https://github.com/pnpm/pnpm/issues/15011).

- A signal sent to pnpm while it runs without a terminal, as a container runtime or a service manager does, now reaches the script even when the shell running it stays the script's parent. pnpm then waits for the script to finish shutting down. Such a signal used to end the shell at once or stay with it, and the script was never told to stop [#7374](https://github.com/pnpm/pnpm/issues/7374).

- Fixed `minimumReleaseAge` being skipped for packages served by a registry that returns the same ETag for abbreviated and full package metadata [pnpm/pnpm#14925](https://github.com/pnpm/pnpm/issues/14925).

- `pnpm install` now returns "Already up to date" in a workspace where `dedupeDirectDeps` left a project without a `node_modules` directory of its own. Such a project forced a full install on every run.

- Installs in different projects that share a global virtual store no longer fail on Windows with `Access is denied` while repairing the same slot [#15114](https://github.com/pnpm/pnpm/issues/15114).

- `pnpm sbom` now emits a license value as a CycloneDX expression only when it is a valid SPDX license expression. Anything else is emitted as a CycloneDX license name [pnpm/pnpm#14786](https://github.com/pnpm/pnpm/issues/14786).

- A dependency's own bins can no longer take over another package's bin shim. The POSIX shims pnpm generates used to look up their shell helpers on `PATH`, where a dependency's bins come first [#14837](https://github.com/pnpm/pnpm/issues/14837). Reinstalling replaces the shims already in your `node_modules`. On Cygwin, MSYS2, and WSL the shims still take their Windows path conversion from `PATH`, so a dependency can still redirect them there.

- POSIX bin shims now convert a Windows-form path such as `C:\node_modules\.bin\tsc` correctly. The shim mangled the backslashes in such a path and could not reach the package it runs. Installing again replaces the shims already in `node_modules` [#14867](https://github.com/pnpm/pnpm/issues/14867).

- `pnpm pack` now writes tarball entries grouped by file extension and file name, the order npm uses. Packages that ship many same-named files, such as template collections, pack much smaller [#14766](https://github.com/pnpm/pnpm/issues/14766).

- A command run in a project that the workspace does not include now acts on that project alone. A project is outside the workspace when it has a manifest of its own and no pattern in the `packages` setting selects it, or when a `!` pattern excludes it. A directory with no manifest of its own, such as a package's source directory, still belongs to the workspace. `pnpm install` in an excluded project used to install every project in the workspace [#3561](https://github.com/pnpm/pnpm/issues/3561).

- POSIX bin shims now take `cygpath` and `wslpath` from the system default path on Cygwin, MSYS2, and WSL2. The shims looked both helpers up on `PATH`, where a dependency's own bins come first, so a dependency could redirect another package's shim. Installing again replaces the shims already in `node_modules` [#14866](https://github.com/pnpm/pnpm/issues/14866).

- `pnpm update` and `pnpm audit --fix=update` no longer copy dependencies added by `packageExtensions`, a `readPackage` hook, or an override into `package.json`. Those dependencies keep the specifier the hook or override gives them. `pnpm update --latest` no longer resolves past that specifier. `pnpm audit --fix=update` now warns when one of them pins a vulnerable version. The warning points at `pnpm audit --fix` [#14928](https://github.com/pnpm/pnpm/issues/14928).
