## 11.28.4

### Patch Changes

- `pnpm install --frozen-lockfile` now succeeds in a project with no dependencies when `pnpm-lock.yaml` records only the pinned pnpm version. Other commands write such a lockfile when they run before the first install. A lockfile missing the `---` line after that section is accepted too [#16477](https://github.com/pnpm/pnpm/issues/16477).

- The `[<since>]` filter selector works again with Git 2.24 through 2.27 [#16561](https://github.com/pnpm/pnpm/issues/16561). With Git older than 2.24, the selector now fails with an error that names the required Git version.

- `pnpm rebuild` and `pnpm approve-builds` refresh command launchers when a build changes a command's interpreter or replaces it with a native executable.

  Dependent packages' build scripts use the refreshed launchers.

- `pnpm install --frozen-lockfile` again succeeds when a workspace project recorded in `pnpm-lock.yaml` has no directory, such as a project left out of a Docker build context. It still fails if the project's directory exists without a `package.json` [#16453](https://github.com/pnpm/pnpm/issues/16453).

- `pnpm install --frozen-lockfile` no longer fails with `ERR_PNPM_OUTDATED_LOCKFILE` for a workspace project that declares `dependenciesMeta` and whose dependencies are all workspace links. pnpm now records that project's `dependenciesMeta` in `pnpm-lock.yaml` [#16457](https://github.com/pnpm/pnpm/issues/16457).

- With `enableGlobalVirtualStore` on, scripts can run entry points that a CommonJS require hook loads again, such as `ts-node index.ts`. They failed with `ERR_UNKNOWN_FILE_EXTENSION` on Node.js versions without built-in TypeScript support [#16436](https://github.com/pnpm/pnpm/issues/16436).

- `pnpm install` with `nodeLinker: hoisted` now refreshes directories supplied by custom fetchers when reinstalling.

- With `nodeLinker: hoisted`, a filtered install of a workspace project no longer fails with `ERR_PNPM_LOCKFILE_MISSING_DEPENDENCY` after a filtered install of another project.

- With `nodeLinker: hoisted`, a filtered install now keeps the packages of the workspace projects an earlier install put in `node_modules`. This also covers the install that `pnpm --filter <selector> run` and `pnpm --filter <selector> exec` start before the command. Before, these installs removed every package that only the unselected projects needed [#16483](https://github.com/pnpm/pnpm/issues/16483).

- `pnpm self-update` now fails for Homebrew-installed pnpm and prints the `brew upgrade` command for the installed formula, such as `brew upgrade pnpm` or `brew upgrade pnpm@11`. It used to install a second copy of pnpm that the Homebrew one kept shadowing [#16547](https://github.com/pnpm/pnpm/issues/16547).

- `pnpm login` no longer forwards credentials in its request body to another origin during redirects.

- Scripts run without a terminal no longer start a second `sh` each. One watchdog per pnpm command now ends every script's process group if pnpm is killed, so `pnpm -r run` across many projects starts half as many processes [#16489](https://github.com/pnpm/pnpm/issues/16489).

- `pnpm -r pkg get` now reports every selected project when several share a package name. Projects with the same name are keyed by their directory relative to the workspace root. Before, only one of them appeared in the output.

- Fixed frozen installs replacing a hoisted dependency with a workspace package of the same name. A later `pnpm dedupe` then removed the hoisted link [#16485](https://github.com/pnpm/pnpm/issues/16485).

- A `devEngines.packageManager` range now records the running pnpm in `pnpm-lock.yaml` only if it meets `minimumReleaseAge`. Otherwise pnpm records the newest version in the range that meets it. If no version in the range does, pnpm still records the running pnpm [#16431](https://github.com/pnpm/pnpm/issues/16431).

- The error for a tarball that fails its integrity check no longer prints credentials, query strings, or fragments from the tarball URL.

- Cached metadata for a package published within `minimumReleaseAge` is now revalidated with its ETag, so the npm registry can answer `304 Not Modified`. Before, the next install that checked the cache downloaded the whole document again.

- pnpm no longer downloads every packument again on each install from a registry whose metadata responses forbid caching, such as `Cache-Control: no-store`. pnpm revalidates the cached metadata with a conditional request, so a registry that supports conditional requests answers with a 304 when the package has not changed [#16528](https://github.com/pnpm/pnpm/issues/16528).

- `pnpm run` and `pnpm exec` now warn and run the command when the install that `verifyDepsBeforeRun` starts fails. This lets scripts run in sandboxes where pnpm cannot install, such as containers with a read-only store or no network [#15173](https://github.com/pnpm/pnpm/issues/15173).

- On Windows, `pnpm self-update` now replaces a `pnpm.exe` left in `PNPM_HOME` or in `PNPM_HOME\bin`. In `PNPM_HOME`, that executable kept running the old version after a successful update. In `PNPM_HOME\bin`, the update failed with `EPERM`. If the executable was in `PNPM_HOME`, `self-update` now asks you to run `pnpm setup` [#9094](https://github.com/pnpm/pnpm/issues/9094).

- Fixed frozen installs creating symlinks to the working directory for skipped optional dependencies and unresolved peer dependencies [#16454](https://github.com/pnpm/pnpm/issues/16454).

- When an optional dependency fails to build, pnpm now removes its link from `node_modules`. A repeat `pnpm install` then reports "Already up to date" and no longer reruns the failing build [#16468](https://github.com/pnpm/pnpm/issues/16468).

- A fetch timeout while other downloads from the same host are still running now lowers concurrency for that host to one connection. Retries of that request, and later downloads from that host, use the lower concurrency. Other hosts keep the configured concurrency [pnpm/pnpm#12791](https://github.com/pnpm/pnpm/issues/12791).

- `pnpm store path`, `pnpm store status`, and other commands that look up the default store no longer fail when the current directory is not writable. pnpm now uses the store in the pnpm home directory in that case [#16554](https://github.com/pnpm/pnpm/issues/16554).

- pnpm now reports an `INVALID_SETTING` error when `allowUnusedPatches` in `pnpm-workspace.yaml` is not a boolean. A quoted value such as `"false"` was treated as `true`.

- pnpm now reports an `INVALID_SETTING` error when `ignoredOptionalDependencies` or `requiredScripts` in `pnpm-workspace.yaml` is not an array of strings.

- A filtered `pnpm run` or `pnpm exec` now finds dependencies out of date when a workspace dependency of a selected project has no `node_modules` directory, as after a filtered install. With `verifyDepsBeforeRun: install`, pnpm installs that dependency before running the command ([pnpm/tasks#45](https://github.com/pnpm/tasks/issues/45)).

- `pnpm install` now prints a warning with the error when an optional dependency cannot be fetched and is skipped. The skipped package is no longer linked into `node_modules` as a broken symlink or listed among the added dependencies. The `pnpm:skipped-optional-dependency` log reports the skip with the `fetch_failure` reason [#16514](https://github.com/pnpm/pnpm/issues/16514).

- Package-name filters now support `?` to match one character [pnpm/pnpm#2817](https://github.com/pnpm/pnpm/issues/2817).

- pnpm can now switch to a `packageManager` version below 11 on x64 musl Linux, such as Alpine [#16467](https://github.com/pnpm/pnpm/issues/16467).
