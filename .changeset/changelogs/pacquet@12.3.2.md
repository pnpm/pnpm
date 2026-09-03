## 12.3.2

### Patch Changes

- `pnpm audit --fix update` no longer aborts when a vulnerable package has no safe version inside its declared range [#14508](https://github.com/pnpm/pnpm/issues/14508). The run updates every package it can and lists the rest as remaining.

- `pnpm install` no longer reruns root lifecycle scripts when the global virtual store contains an unfinished-build marker in a package slot that the current lockfile does not use [pnpm/pnpm#14485](https://github.com/pnpm/pnpm/issues/14485).

- Sped up installs that have no lockfile. pnpm now links packages whose dependency subtree has no peer dependencies into the virtual store while resolution is still running.

- `pnpm run` and `pnpm exec` now start without reinstalling on filesystems that keep sub-millisecond mtimes, such as NTFS. Previously, every run on those filesystems reinstalled first [pnpm/pnpm#14486](https://github.com/pnpm/pnpm/issues/14486).

- `pnpm import` now keeps the versions recorded in `package-lock.json`, `npm-shrinkwrap.json`, or `yarn.lock` when it generates `pnpm-lock.yaml`. A range in `package.json`, a catalog, or an override still decides which versions are eligible, and the recorded version is preferred among them. The generated lockfile previously could pin newer versions than the source lockfile [#14476](https://github.com/pnpm/pnpm/issues/14476).

  `pnpm import` in a workspace now imports every workspace project into the shared lockfile. It previously imported only the project in the current directory.

  `pnpm import` now fails with `ERR_PNPM_LOCKFILE_NOT_FOUND` when none of the three source lockfiles is present. It also fails with `ERR_PNPM_YARN_LOCKFILE_PARSE_FAILED` when it cannot parse `yarn.lock`. It previously generated a lockfile from scratch in both cases.

  `pnpm import` always resolves locally. It warns when `--pnpr-server` or the `pnpr-server` setting is given and does not use the server.

- Sped up installs in large workspaces. Discovering the workspace projects no longer enumerates every matched directory to learn which manifest files it holds [#14352](https://github.com/pnpm/pnpm/issues/14352).

- Sped up installs in large workspaces. The resolver and the peer pass allocate less for every dependency edge [#14352](https://github.com/pnpm/pnpm/issues/14352).

- `pnpm self-update`, `pnpm with`, and automatic package-manager version switching no longer wait through registry retry delays when a configured registry has no signatures and `registry.npmjs.org` is unavailable [#14483](https://github.com/pnpm/pnpm/issues/14483).

- Sped up installs in large workspaces. Saving the lockfile is faster, and the install finishes without waiting for memory cleanup [#14352](https://github.com/pnpm/pnpm/issues/14352).

- `pnpm install` now relinks workspace packages when `publishConfig.linkDirectory` changes. Frozen installs report an outdated lockfile until it is regenerated [pnpm/pnpm#14488](https://github.com/pnpm/pnpm/issues/14488).

- The pnpm npm wrapper keeps its placeholder shebang-less so pnpm 11 can install pnpm 12 through the version store. Wrapper installs must allow lifecycle scripts to install the native binary [#14502](https://github.com/pnpm/pnpm/issues/14502).

- Sped up dependency resolution when there is no lockfile, and for the dependencies a lockfile does not cover.

- Sped up installs in large workspaces. Workspace `link:` targets and importer ids are now derived from the paths' suffixes under the workspace root [#14352](https://github.com/pnpm/pnpm/issues/14352).

- `pnpm install` now reports "Already up to date" when local tarball dependencies have not changed [#14495](https://github.com/pnpm/pnpm/issues/14495).

- `pnpm update` now accepts `--ignore-scripts` and skips lifecycle scripts during the update [pnpm/pnpm#14512](https://github.com/pnpm/pnpm/issues/14512).

- Sped up installs that restore a deleted `node_modules` from a warm global virtual store. pnpm no longer re-links packages that are already fully present in the global virtual store [#14510](https://github.com/pnpm/pnpm/issues/14510).
