## 11.28.9

### Patch Changes

- `pnpm audit --fix` and `pnpm audit --interactive` now fail with `ERR_PNPM_AUDIT_FIX_WITH_IGNORE` when combined with `--ignore` or `--ignore-unfixable`. Before, the fix ran and the ignore flags had no effect.

- `pnpm audit --ignore` and `pnpm audit --ignore-unfixable` now exit with a non-zero code when vulnerabilities at or above the audit level remain after ignoring. Before, these commands always exited with code 0.

- `pnpm -r publish --batch` now sends `PUT /-/pnpr/v0/publish`, the batch publish endpoint that pnpr serves.

- The proxy settings `httpProxy`, `httpsProxy`, `noProxy`, `proxy`, and `noproxy` set in the global `config.yaml`, through `PNPM_CONFIG_HTTPS_PROXY` and the matching variables, or as `--config.httpsProxy` on the command line now apply to `pnpm self-update` and to the download of the pnpm version pinned in `packageManager`. Previously, these requests read the proxy only from `.npmrc` and the `HTTPS_PROXY` family of variables [pnpm/tasks#215](https://github.com/pnpm/tasks/issues/215).

- `strictSsl` and `localAddress` set in the global `config.yaml`, through `PNPM_CONFIG_STRICT_SSL` and `PNPM_CONFIG_LOCAL_ADDRESS`, or on the command line now apply to `pnpm self-update` and to the download of the pnpm version pinned in `packageManager`. These requests read them from `.npmrc` only before. `--config.localAddress` now reaches them in the camelCase spelling too [pnpm/tasks#201](https://github.com/pnpm/tasks/issues/201).

- `pnpm pack` and `pnpm publish` again include the files of a `publishConfig.directory` that is reached through a symlink, together with its bundled dependencies [pnpm/pnpm#17390](https://github.com/pnpm/pnpm/issues/17390).

- `pnpm ci --lockfile` and `pnpm ci --config.lockfile=true` no longer delete `pnpm-lock.yaml`. `pnpm ci` now always keeps the lockfile and runs a frozen install from it.

- `pnpm deploy` now keeps a deployed symlink to a directory inside the project even if the directory is not among the deployed files, such as a link into `node_modules` [#17288](https://github.com/pnpm/pnpm/issues/17288).

- `pnpm install` now applies the `serverType` registry setting when it updates the lockfile for an override or catalog change without a full resolution. pnpm wrote the tarball URLs of a declared registry into the lockfile in that case.

- An install filtered to some workspace projects now fails if `node_modules` was installed with a different `nodeLinker`, `hoistPattern`, `publicHoistPattern`, store, or virtual store. Before, it recreated `node_modules` and left the projects it skipped without their dependencies. A `pnpm install` of the whole workspace still recreates `node_modules` [pnpm/tasks#217](https://github.com/pnpm/tasks/issues/217).

- With `nodeLinker: hoisted`, an install that stops partway no longer leaves the next install trusting the packages it was replacing. Before, reverting the change that started the stopped install, such as a patch edit, kept the half-replaced package in `node_modules`.

- With `injectWorkspacePackages: true`, a `workspace:` dependency written as a relative path, such as `workspace:../lib-a`, in a workspace package now resolves from that package's directory [#17284](https://github.com/pnpm/pnpm/issues/17284).

- An optional dependency written with a named-registry prefix, such as `work:^1.0.0`, now fails the install when it cannot be resolved but the lockfile already has a matching entry from that registry. Such a dependency was skipped, which removed its entries from the lockfile.

- `pnpm install` now resolves a scope's packages again when the scope is routed to another registry in `registries`. Before, the lockfile kept the versions and integrity resolved from the old registry, so the install failed when the new registry did not serve a locked version or served different content. pnpm records in the workspace state which registry each scope routed to, so a fresh clone without that record installs the lockfile as recorded. Changing the default registry resolves nothing again, and `pnpm install --frozen-lockfile` is unchanged.

- `pnpm install` now takes its fast path in a workspace with `patchedDependencies` when the previous install ran with another major version of pnpm.

- Installs delegated to a pnpr server now resolve every prefix declared for a registry URL. A dependency or lockfile entry that used the second of two prefixes for the same registry, such as `zeta:` next to `work:` in `namedRegistries`, failed on the server [pnpm/tasks#204](https://github.com/pnpm/tasks/issues/204).

- Added the `pnpr` value to the `serverType` registry setting. pnpm leaves the tarball URLs of a pnpr registry out of `pnpm-lock.yaml`.

- When `https-proxy` is `false`, pnpm now downloads the pnpm version pinned by `packageManager` through the legacy `proxy` setting, as installs already do. Setting `proxy` to `false` now also stops that download from using the `HTTP_PROXY` environment variable. The signature check of the pinned pnpm now uses the same proxy and TLS settings as its download [pnpm/tasks#193](https://github.com/pnpm/tasks/issues/193).

- `pnpm install` now removes command shims from `node_modules/.bin` when a workspace dependency renames or removes an entry from its `bin` field [pnpm/tasks#211](https://github.com/pnpm/tasks/issues/211).

- Sped up installs of projects whose local directory dependencies share transitive local directory dependencies. pnpm no longer checks a shared directory again for each direct dependency that reaches it [#17374](https://github.com/pnpm/pnpm/issues/17374).

- `pnpm add`, `pnpm remove`, and `pnpm update` now fail when the `node-linker` setting differs from the one recorded in `node_modules`. A `pnpm install` of the whole workspace recreates `node_modules` with the new `node-linker`, and an install filtered to some projects fails [pnpm/tasks#203](https://github.com/pnpm/tasks/issues/203).

- A package that a plain range and a `<prefix>:` specifier both resolve from one registry is now recorded once in `pnpm-lock.yaml` and installed once. The lockfile key names the registry a package resolved from. When that registry declares a prefix, through `prefix` in `registries` or through `namedRegistries`, the package is keyed `<name>@<prefix>:<version>` however its dependency was written: with the prefix, as a plain range its scope routes to that registry, or through the default registry. Before, the plain range was keyed `<name>@<version>`, so one tarball was installed twice and `instanceof` checks and module singletons broke between the two copies. The same name and version served by two registries still get separate entries. When several prefixes declare one registry, its packages are keyed by the lexicographically smallest prefix.

  If a scope you route reaches a registry that also declares a prefix, your next `pnpm install` re-keys those entries, which shows up as a lockfile diff. Commit it. `pnpm install --frozen-lockfile` installs the lockfile as recorded.

- A `registries` entry whose `prefix` is `__proto__` is no longer ignored.

- `pnpm install` now recreates `node_modules` after a `storeDir` or `virtualStoreDir` change. It used to report "Already up to date".

- Fixed `pnpm install` failing with `ERR_PNPM_UNSUPPORTED_PROTOCOL` when a workspace package injected through a relative path, such as `workspace:../lib`, has `catalog:` dependencies. Their catalog entries now resolve the way they do for a package injected by name [#17280](https://github.com/pnpm/pnpm/issues/17280).

- A repeat `pnpm install` now renames the virtual store directories after `virtualStoreDirMaxLength` changes. It now runs the builds it skipped before after `dangerouslyAllowAllBuilds` is turned on. It now verifies the lockfile after `trustLockfile` is turned off. Before, it reported "Already up to date" in each of these cases.

- `pnpm store prune` now keeps the packages that a project still uses when pnpm copied or cloned them into `node_modules`. These include patched packages, built packages, and every package installed with `packageImportMethod: copy`. Before, the prune removed them, and the next install downloaded them again [#7192](https://github.com/pnpm/pnpm/issues/7192).

- `pnpm store prune` now keeps the packages of a project that was installed with a different symlinked spelling of the store path. Previously, prune dropped such a project from its list and removed the packages only that project used from the global virtual store.

- Output that fills the terminal, such as `pnpm run --stream` script output, is now written once. Before, every new line also rewrote all the lines already on screen [#17387](https://github.com/pnpm/pnpm/issues/17387).

- The warning about unrecognized settings now marks `symlinkedLockfileTargetDirs` as a setting of a newer pnpm version.

- A registry response with too many content encodings can no longer crash pnpm. pnpm now uses undici 7.30.1, which fixes [GHSA-xjj6-f33q-929c](https://github.com/advisories/GHSA-xjj6-f33q-929c).

- `pnpm run` and `pnpm exec` no longer report the dependencies as out of date after a lockfile change when `dedupePeers` is `true` [#17322](https://github.com/pnpm/pnpm/issues/17322).

- `pnpm install` now sizes its worker pool by the CPU quota of the container it runs in, such as a Kubernetes CPU limit, when that quota is smaller than the number of CPUs the container may run on. The default `workspaceConcurrency` respects the quota the same way. On a CI runner with a 4-CPU quota and hundreds of host CPUs, pnpm started hundreds of workers and ran out of memory [#17382](https://github.com/pnpm/pnpm/issues/17382).

- `pnpm install` now relinks the bins of a workspace package when its `bin` field changes, including with `--frozen-lockfile` and an unchanged lockfile. A filtered install relinks them in the selected projects, and the next install that covers the remaining projects relinks them there [pnpm/pnpm#17324](https://github.com/pnpm/pnpm/issues/17324).
