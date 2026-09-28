## 1102.1.5

### Patch Changes

- `pnpm setup` no longer writes the `pn.ps1`, `pnpx.ps1`, and `pnx.ps1` PowerShell wrappers. It also removes the ones an earlier setup wrote. PowerShell now runs `pn`, `pnpx`, and `pnx` through their `.cmd` wrappers, like `pnpm` itself. Before, these aliases failed with a "not digitally signed" error wherever the execution policy blocks unsigned scripts [#8444](https://github.com/pnpm/pnpm/issues/8444).

- A signal sent to pnpm, such as `SIGTERM`, now reaches the pnpm that pnpm switches to because of `packageManager` or `devEngines.packageManager`, and the one that `pnpm with` runs. The signal used to be dropped, so scripts running under that pnpm never got to shut down [#9948](https://github.com/pnpm/pnpm/issues/9948).

- `pnpm self-update` no longer suggests a downgrade when `minimumReleaseAge` holds back the registry's `latest` release. It now says that release is still within the cutoff [#12006](https://github.com/pnpm/pnpm/issues/12006).

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.34
  - @pnpm/building.policy@1100.1.4
  - @pnpm/cli.utils@1101.0.30
  - @pnpm/config.reader@1102.3.1
  - @pnpm/config.version-policy@1100.2.5
  - @pnpm/deps.graph-hasher@1100.3.5
  - @pnpm/deps.security.signatures@1102.0.6
  - @pnpm/error@1100.2.1
  - @pnpm/exec.npm-lifecycle@1100.0.2
  - @pnpm/global.commands@1102.0.4
  - @pnpm/global.packages@1101.1.5
  - @pnpm/installing.client@1100.3.12
  - @pnpm/installing.deps-restorer@1103.2.1
  - @pnpm/installing.env-installer@1103.0.7
  - @pnpm/lockfile.fs@1100.2.10
  - @pnpm/lockfile.types@1100.1.4
  - @pnpm/network.auth-header@1101.1.15
  - @pnpm/pkg-manifest.utils@1100.4.7
  - @pnpm/resolving.npm-resolver@1104.2.2
  - @pnpm/store.connection-manager@1101.3.0
  - @pnpm/store.controller@1102.2.1
  - @pnpm/workspace.project-manifest-reader@1100.1.1
