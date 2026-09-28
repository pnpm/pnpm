## 1102.1.5

### Patch Changes

- `pnpm audit` and `pnpm audit signatures` now fail with an error when the lockfile contains unresolvable dependency references [pnpm/pnpm#13638](https://github.com/pnpm/pnpm/issues/13638).

- `pnpm licenses list --json` now includes every installed copy of a package in its `paths` array. Multiple copies of the same version previously contributed only one path. This includes hoisted copies and isolated installations with different peer dependencies.

- `pnpm licenses list` now reports the actual on-disk package locations when using `nodeLinker: hoisted` or `shamefully-hoist: true` [pnpm/pnpm#8589](https://github.com/pnpm/pnpm/issues/8589).

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.30
  - @pnpm/config.reader@1102.3.1
  - @pnpm/config.version-policy@1100.2.5
  - @pnpm/config.writer@1100.0.29
  - @pnpm/deps.compliance.audit@1101.0.39
  - @pnpm/deps.compliance.license-scanner@1101.0.7
  - @pnpm/deps.compliance.sbom@1101.0.6
  - @pnpm/deps.security.signatures@1102.0.6
  - @pnpm/error@1100.2.1
  - @pnpm/installing.commands@1101.4.1
  - @pnpm/lockfile.fs@1100.2.10
  - @pnpm/lockfile.types@1100.1.4
  - @pnpm/lockfile.utils@1102.1.5
  - @pnpm/lockfile.walker@1100.0.26
  - @pnpm/network.auth-header@1101.1.15
  - @pnpm/network.fetch@1100.1.19
  - @pnpm/pkg-manifest.utils@1100.4.7
  - @pnpm/store.path@1100.1.0
  - @pnpm/workspace.project-manifest-reader@1100.1.1
