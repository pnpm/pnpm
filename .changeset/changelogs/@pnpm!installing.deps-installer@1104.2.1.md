## 1104.2.1

### Patch Changes

- The lockfile verification error now suggests relaxing the policy that flagged an entry only if a fresh resolution still fails and you trust the affected packages. Errors from checks that no policy controls, such as a missing tarball integrity, no longer suggest relaxing a policy [#14411](https://github.com/pnpm/pnpm/issues/14411).

- `pnpm install --dev` and `pnpm fetch --dev` now install the optional dependencies of devDependencies, such as the platform binaries of Biome and oxlint. The project's own `optionalDependencies` are still skipped [#9678](https://github.com/pnpm/pnpm/issues/9678).

- The global virtual store and the side-effects cache now key built packages by the Node.js version that the root project's `devEngines.runtime` or `engines.runtime` pins. That is the Node.js their build scripts run with. A dependency that declares its own `engines.runtime` no longer changes the key for every other package.

- `pnpm install` now restores cached build artifacts when reinstalling a workspace that uses separate lockfiles [#12942](https://github.com/pnpm/pnpm/issues/12942).

- `pnpm install` now repairs a `pnpm-lock.yaml` whose `(patch_hash=<hash>)` dependency paths disagree with its `patchedDependencies` map, including paths that lack the hash their patch calls for. pnpm previously accepted such a lockfile as up to date and kept the old patched files. `pnpm install --frozen-lockfile` now fails on such a lockfile with `ERR_PNPM_INCONSISTENT_PATCH_HASH`. It fails with `ERR_PNPM_UNCHECKABLE_PATCH_HASH` when a patch hash in the lockfile is malformed, or when the lockfile lacks the package version or patch entry that the check needs [#15336](https://github.com/pnpm/pnpm/pull/15336).

- Fixed `pnpm rebuild` modifying packages shared with projects that have not approved their build scripts when using the global virtual store.

- `pnpm import` in a workspace now keeps the versions pinned by a `yarn.lock` inside a workspace project [#4385](https://github.com/pnpm/pnpm/issues/4385).

- `pnpm install` no longer fails for an injected workspace dependency whose package publishes from a `publishConfig.directory` that its own `prepare` script builds. The injected copy now picks up that directory once `prepare` finishes building it. `pnpm install --frozen-lockfile` no longer reports the dependency as outdated while the directory has not been built yet. [pnpm/pnpm#7811](https://github.com/pnpm/pnpm/issues/7811)

- With `sharedWorkspaceLockfile: false`, an injected workspace package that has lifecycle scripts is now hard linked into the projects that depend on it. Before, pnpm left a plain copy, so later edits to the package did not reach those projects [#9828](https://github.com/pnpm/pnpm/issues/9828).

- `pnpm install` no longer fails when a package from the registry declares a `file:` dependency on a directory inside itself, such as `"@types/css-tree": "file:./typings/css-tree"`. pnpm links that dependency to the directory inside the package, as npm and Yarn do. The lockfile records it as `link:<root>/typings/css-tree` [#9141](https://github.com/pnpm/pnpm/issues/9141).

- `pnpm install` now fails with `ERR_PNPM_PATCH_NOT_FOUND` when a patch file listed in `patchedDependencies` does not exist. It used to fail with a raw `ENOENT` error and a stack trace [#5268](https://github.com/pnpm/pnpm/issues/5268).

- `engineStrict` now checks the patched `package.json` when a `patchedDependencies` entry changes `engines`. A patch that relaxes `engines.node` no longer fails the install against the published range [pnpm/pnpm#9603](https://github.com/pnpm/pnpm/issues/9603).

- Installing with `pnprServer` set now records the pnpmfile checksum in the lockfile, so a later `pnpm install --frozen-lockfile` accepts that lockfile. A frozen install through the pnpr server now fails if the pnpmfile changed. If the pnpmfile defines a `readPackage`, `afterAllResolved` or `preResolution` hook or custom resolvers, pnpm resolves dependencies locally. pnpm then prints a warning that the pnpr server was not used [#14460](https://github.com/pnpm/pnpm/issues/14460).

- Installing through a `pnpr` server now links a workspace project at the directory its `publishConfig.directory` names, instead of linking the project root. An install that resolves through a server which does not forward the setting fails with `ERR_PNPM_PNPR_PUBLISH_DIRECTORY_MISMATCH` instead of writing a lockfile that points at the wrong directory, and the server rejects a `publishConfig.directory` that points outside its project [#14460](https://github.com/pnpm/pnpm/issues/14460).

- `pnpm install` no longer creates a `node_modules` symlink inside the `publishConfig.directory` of a workspace package linked with `linkDirectory`. A build tool that cleaned its output directory through that symlink deleted the files of the package's dependencies. `pnpm install` also removes a symlink that an earlier install left there [#16226](https://github.com/pnpm/pnpm/issues/16226).

- `pnpm update` now applies an override that references a catalog with the catalog's new value when the update bumps that catalog entry. Before, the packages the override targets kept the old version in the lockfile [#12159](https://github.com/pnpm/pnpm/issues/12159).

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.34
  - @pnpm/bins.remover@1100.0.26
  - @pnpm/building.after-install@1103.0.7
  - @pnpm/building.during-install@1102.2.4
  - @pnpm/building.policy@1100.1.4
  - @pnpm/catalogs.config@1100.0.9
  - @pnpm/catalogs.resolver@1100.1.1
  - @pnpm/config.package-is-installable@1100.2.1
  - @pnpm/config.parse-overrides@1100.1.7
  - @pnpm/config.version-policy@1100.2.5
  - @pnpm/crypto.hash@1100.0.7
  - @pnpm/deps.graph-hasher@1100.3.5
  - @pnpm/deps.path@1101.0.5
  - @pnpm/error@1100.2.1
  - @pnpm/exec.lifecycle@1100.1.20
  - @pnpm/fs.read-modules-dir@1100.0.3
  - @pnpm/fs.symlink-dependency@1100.0.22
  - @pnpm/hooks.read-package-hook@1100.3.5
  - @pnpm/hooks.types@1101.0.5
  - @pnpm/installing.context@1101.0.7
  - @pnpm/installing.deps-resolver@1102.2.4
  - @pnpm/installing.deps-restorer@1103.2.1
  - @pnpm/installing.linking.direct-dep-linker@1100.0.22
  - @pnpm/installing.linking.hoist@1100.0.34
  - @pnpm/installing.linking.modules-cleaner@1100.1.26
  - @pnpm/installing.modules-yaml@1101.0.5
  - @pnpm/installing.package-requester@1102.2.1
  - @pnpm/lockfile.filtering@1100.2.10
  - @pnpm/lockfile.fs@1100.2.10
  - @pnpm/lockfile.preferred-versions@1100.0.36
  - @pnpm/lockfile.pruner@1100.0.26
  - @pnpm/lockfile.settings-checker@1100.2.9
  - @pnpm/lockfile.to-pnp@1101.0.7
  - @pnpm/lockfile.utils@1102.1.5
  - @pnpm/lockfile.verification@1100.1.8
  - @pnpm/lockfile.walker@1100.0.26
  - @pnpm/network.auth-header@1101.1.15
  - @pnpm/patching.config@1100.1.8
  - @pnpm/pkg-manifest.utils@1100.4.7
  - @pnpm/pnpr.client@3.1.0
  - @pnpm/resolving.local-resolver@1101.2.4
  - @pnpm/resolving.npm-resolver@1104.2.2
  - @pnpm/resolving.parse-wanted-dependency@1100.0.3
  - @pnpm/resolving.resolver-base@1101.3.2
  - @pnpm/store.controller-types@1101.3.2
  - @pnpm/store.index@1100.3.3
  - @pnpm/workspace.project-manifest-reader@1100.1.1
