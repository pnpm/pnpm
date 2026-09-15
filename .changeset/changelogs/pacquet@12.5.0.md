## 12.5.0

### Minor Changes

- `pnpm install --frozen-lockfile` now replays a `pylock.toml` on any Python target that can install it. Previously the lockfile was reused only for the exact marker environment and wheel tag order that produced it, so a kernel update alone invalidated it. A lockfile is now reused when the project's requirements, index and `requires-python` are unchanged, every pinned wheel carries tags the interpreter accepts, and the locked packages are exactly what the interpreter's markers select [#14843](https://github.com/pnpm/pnpm/issues/14843).

  The `environments` marker written to `pylock.toml` now names only the interpreter version and the marker variables the locked dependency graph reads. When the lockfile is not frozen and its locked graph no longer matches the target, `pnpm install` warns and resolves the project again.

### Patch Changes

- `pn`, `pnpx`, and `pnx` now run the pnpm installed alongside them. They used to look pnpm up on `PATH`. That failed when the directory holding them was not on `PATH`, and it silently handed the call to an unrelated pnpm when one came first there [#14803](https://github.com/pnpm/pnpm/issues/14803).

- `pnpm add <git repository>` now names a repository that ships no `package.json` after its owner and repository, such as `@owner/repo`. Two repositories that share a repository name can be dependencies of one project [pnpm/pnpm#14870](https://github.com/pnpm/pnpm/issues/14870).

- `pnpm install` in a single-project directory now detects a `package.json` edit that landed while the previous install was still finishing. Such an edit was reported as "Already up to date" while `pnpm install --frozen-lockfile` rejected the same working tree [#14890](https://github.com/pnpm/pnpm/issues/14890).

- Fixed `pnpm install` on macOS laying out an earlier install's files for a `file:` tarball or a git-hosted tarball dependency.

- `pnpm --filter "./packages/{app,lib}"` now selects either alternative. Brace alternatives nest, may span a path separator, and combine with the other wildcards.

- `pnpm deploy` now writes plain versions for registry dependencies with peer dependencies in the deployed `package.json`. The deployed lockfile retains the resolved peer bindings. npm aliases keep their target package names [#14873](https://github.com/pnpm/pnpm/issues/14873).

- Cargo and Python discovery now skip directories excluded by `!` patterns in `pnpm-workspace.yaml` `packages`. Excluded native projects are no longer parsed. They no longer receive generated source configuration [pnpm/pnpm#14844](https://github.com/pnpm/pnpm/issues/14844).

- Resolving a Node.js runtime now fails when unofficial-builds.nodejs.org cannot be reached. pnpm used to ignore that failure and leave the musl builds out of `pnpm-lock.yaml`. `pnpm update` then wrote a different lockfile on a machine whose network blocks the mirror [pnpm/pnpm#14813](https://github.com/pnpm/pnpm/issues/14813).

- pnpm now deduplicates a package whose child dependency resolved an optional peer in one workspace project but not in another. Two copies of `next` could appear when only some projects could reach `styled-jsx`'s optional `babel-plugin-macros` peer [#14800](https://github.com/pnpm/pnpm/issues/14800).

- `pnpm sbom` now publishes a valid URL in the CycloneDX `externalReferences[].url` and the SPDX `homepage`. An npm shorthand such as `vercel/ms` or `gitlab:group/subgroup/project` is expanded to the `git+https` URL npm derives for it. An scp-style remote such as `git@github.com:vercel/ms.git` is expanded the same way. Any other URL is published in its normalized form, without embedded credentials. A value that names no repository, an email address for example, is left out. pnpm used to publish the raw value, so a shorthand produced a URL that consumers such as Dependency-Track reject [pnpm/pnpm#14773](https://github.com/pnpm/pnpm/issues/14773).

- `pnpm install --frozen-lockfile` now removes a package from `node_modules` when no project in `pnpm-lock.yaml` depends on it any more. Such a package used to be reinstalled on every run, which reran the lifecycle scripts each time. `verifyDepsBeforeRun` also installed before every `pnpm run` and `pnpm exec` [#14891](https://github.com/pnpm/pnpm/issues/14891).

- `pnpm --version` now reports why the pnpm version a project pins cannot be installed or recorded, then prints the version of the running CLI. It used to fail, which made the command unusable where the filesystem is read-only. `pnpm --version` also honors `--store-dir` and its `--store` alias now [#14831](https://github.com/pnpm/pnpm/issues/14831).

- pnpm no longer crashes at startup on FreeBSD and other Unix-like platforms. The default store directory is `~/.local/share/pnpm/store` on every platform that is not Windows or macOS [#14859](https://github.com/pnpm/pnpm/issues/14859).

- GitHub Actions updates now stop if an action reference changes while its versions are being resolved. Unrelated workflow edits are preserved.

  GitHub Actions homepage links no longer expose server credentials. GitHub server URLs now require HTTPS, with HTTP allowed only for loopback hosts.

- Sped up frozen-lockfile hoisted installs on macOS when reusable package directories are already cached.

- `pnpm install` and `pnpm update` now honor `--ignore-workspace` in a project nested under a workspace root but excluded from its `packages` patterns. They previously installed every project of the surrounding workspace. `pnpm-lock.yaml` was written at the workspace root. A build script belonging to that root failed the command with `ERR_PNPM_IGNORED_BUILDS`. The flag now also covers the `packageManager` check that runs before every command, which failed with `ERR_PNPM_UNRECOGNIZED_WORKSPACE_SETTINGS` on an unrecognized setting in the ignored file [#14809](https://github.com/pnpm/pnpm/issues/14809).

- `pnpm install <pkg>` now accepts `--prod` and `--dev`, including the `--prod=false` spelling. Those flags used to abort the command with an argument parsing error [#14868](https://github.com/pnpm/pnpm/issues/14868).

- Sped up installs and `pnpm peers check` in workspaces whose projects depend on each other. A workspace package's unmet peer dependencies are now reported only under the projects that link it directly. Since 12.3.0 a lockfile update in such a workspace took around ten times longer, and `pnpm peers check` could run for many minutes [#14906](https://github.com/pnpm/pnpm/issues/14906).

- add missing `t` and `tst` aliases for `test` CLI command

- `pnpm update --no-save` no longer rewrites the specifier of a dependency it is not updating. An override-applied specifier was replaced with the range the manifest declares, and the next `pnpm install --frozen-lockfile` failed with `ERR_PNPM_OUTDATED_LOCKFILE` [#14836](https://github.com/pnpm/pnpm/issues/14836).

- On Windows, `pnpm install` no longer fails with `ERR_PNPM_PACKAGE_MANAGER_REMOVE_MODULES_DIR` when it clears a `node_modules` directory that holds linked dependencies. pnpm could not remove a linked dependency there, so changing `nodeLinker` in an installed project aborted the install [#14790](https://github.com/pnpm/pnpm/issues/14790).

- `pnpm add pypi:...` now rejects an unsupported `--save-prefix` before editing the Python manifest or resolving dependencies.

- A script listed in `syncInjectedDepsAfterScripts` no longer fails with `ERR_PNPM_INJECTED_DEPS_SYNC_READ_DIR` in a workspace whose lockfile carries an injected copy of a package that no project depends on. `pnpm install` used to record that copy in `node_modules/.modules.yaml` even though it did not keep it.

- `pnpm self-update` no longer reinstalls pnpm when the active version was installed by the standalone installation script [#14823](https://github.com/pnpm/pnpm/issues/14823).

- Reduced memory overhead during hoisted installs when packages are already cached.

- Scripts run under `shellEmulator` now expand the braced parameter forms `${VAR}`, `${VAR:-default}`, and `${VAR:+alternative}`. They were passed to the script as literal text [#14814](https://github.com/pnpm/pnpm/issues/14814).

- A dependency's own bins can no longer take over another package's bin shim. The POSIX shims pnpm generates used to look up their shell helpers on `PATH`, where a dependency's bins come first [#14837](https://github.com/pnpm/pnpm/issues/14837). Reinstalling replaces the shims already in your `node_modules`. On Cygwin, MSYS2, and WSL the shims still take their Windows path conversion from `PATH`, so a dependency can still redirect them there.

- Python resolution no longer fails on a release whose `Requires-Python` is not a version specifier, such as the trailing comma in `openpyxl` 3.0.x. pnpm now reads such a value as if the release declared no interpreter range [#14910](https://github.com/pnpm/pnpm/issues/14910).

- `pnpm update --no-save` no longer fails under `minimumReleaseAgeStrict` when every version it resolves is old enough. The command is refused only when it picks a version younger than the cutoff [#14835](https://github.com/pnpm/pnpm/issues/14835).

- `pnpm update` now settles the lockfile in one run when an upgrade removes the package that provided an optional peer dependency. A second `pnpm update` used to change the lockfile again, with no version change in the diff [#14895](https://github.com/pnpm/pnpm/issues/14895).

- `pnpm install` now fails with `ERR_PNPM_INVALID_PEER_DEPENDENCY_SPECIFICATION` when a `peerDependencies` value is neither a version range nor a valid specifier. A typo such as `"foo": "foo@1.0.0"` used to install as a directory link, leaving a broken symlink in `node_modules` [#14791](https://github.com/pnpm/pnpm/issues/14791).
