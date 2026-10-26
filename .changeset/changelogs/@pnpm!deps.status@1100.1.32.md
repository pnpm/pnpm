## 1100.1.32

### Patch Changes

- `pnpm install` now resolves a scope's packages again when the scope is routed to another registry in `registries`. Before, the lockfile kept the versions and integrity resolved from the old registry, so the install failed when the new registry did not serve a locked version or served different content. pnpm records in the workspace state which registry each scope routed to, so a fresh clone without that record installs the lockfile as recorded. Changing the default registry resolves nothing again, and `pnpm install --frozen-lockfile` is unchanged.

- `pnpm install` now takes its fast path in a workspace with `patchedDependencies` when the previous install ran with another major version of pnpm.

- A package that a plain range and a `<prefix>:` specifier both resolve from one registry is now recorded once in `pnpm-lock.yaml` and installed once. The lockfile key names the registry a package resolved from. When that registry declares a prefix, through `prefix` in `registries` or through `namedRegistries`, the package is keyed `<name>@<prefix>:<version>` however its dependency was written: with the prefix, as a plain range its scope routes to that registry, or through the default registry. Before, the plain range was keyed `<name>@<version>`, so one tarball was installed twice and `instanceof` checks and module singletons broke between the two copies. The same name and version served by two registries still get separate entries. When several prefixes declare one registry, its packages are keyed by the lexicographically smallest prefix.

  If a scope you route reaches a registry that also declares a prefix, your next `pnpm install` re-keys those entries, which shows up as a lockfile diff. Commit it. `pnpm install --frozen-lockfile` installs the lockfile as recorded.

- `pnpm install` now recreates `node_modules` after a `storeDir` or `virtualStoreDir` change. It used to report "Already up to date".

- A repeat `pnpm install` now renames the virtual store directories after `virtualStoreDirMaxLength` changes. It now runs the builds it skipped before after `dangerouslyAllowAllBuilds` is turned on. It now verifies the lockfile after `trustLockfile` is turned off. Before, it reported "Already up to date" in each of these cases.

- `pnpm run` and `pnpm exec` no longer report the dependencies as out of date after a lockfile change when `dedupePeers` is `true` [#17322](https://github.com/pnpm/pnpm/issues/17322).

- `pnpm install` now relinks the bins of a workspace package when its `bin` field changes, including with `--frozen-lockfile` and an unchanged lockfile. A filtered install relinks them in the selected projects, and the next install that covers the remaining projects relinks them there [pnpm/pnpm#17324](https://github.com/pnpm/pnpm/issues/17324).

- Updated dependencies:
  - @pnpm/config.normalize-registries@1101.0.5
  - @pnpm/config.reader@1102.3.7
  - @pnpm/deps.path@1101.0.8
  - @pnpm/hooks.read-package-hook@1100.3.10
  - @pnpm/installing.context@1101.0.13
  - @pnpm/lockfile.fs@1100.2.15
  - @pnpm/lockfile.settings-checker@1100.2.15
  - @pnpm/lockfile.utils@1102.1.8
  - @pnpm/lockfile.verification@1100.1.14
  - @pnpm/resolving.resolver-base@1101.3.4
  - @pnpm/types@1102.1.3
  - @pnpm/workspace.projects-reader@1101.1.6
  - @pnpm/workspace.state@1100.0.52
  - @pnpm/workspace.workspace-manifest-reader@1100.3.1
