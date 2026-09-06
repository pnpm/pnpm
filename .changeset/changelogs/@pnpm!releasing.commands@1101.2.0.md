## 1101.2.0

### Minor Changes

- Added `pnpm change check`. It validates the committed package versions against the `versioning.epics` bands and the `versioning.fixed` groups in `pnpm-workspace.yaml` and lists every violation. It is meant to run in CI, because `pnpm version -r` only checks the packages it releases.

### Patch Changes

- `pnpm deploy` no longer requires `injectWorkspacePackages` to be enabled. A linked workspace dependency is rewritten to a `file:` dependency in the dedicated deploy lockfile, and the peer dependencies it declares are bound to the deployed graph's own resolution.

  When a peer resolves to more than one version in that graph the binding is ambiguous, and choosing between the candidates is exactly what injecting the package would have decided, so the deploy still fails — now with `ERR_PNPM_DEPLOY_AMBIGUOUS_PEER`, which names the package, the peer, and the competing versions, instead of refusing every non-injected workspace up front, and suggests pinning the peer to one version with an `overrides` entry as the way to keep deploying without injection [#9386](https://github.com/pnpm/pnpm/issues/9386).

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.26
  - @pnpm/config.reader@1102.1.1
  - @pnpm/engine.runtime.commands@1101.1.1
  - @pnpm/engine.runtime.node-resolver@1101.3.0
  - @pnpm/error@1100.1.4
  - @pnpm/exec.lifecycle@1100.1.16
  - @pnpm/fetching.directory-fetcher@1100.0.32
  - @pnpm/installing.client@1100.3.8
  - @pnpm/installing.commands@1101.2.0
  - @pnpm/lockfile.fs@1100.2.6
  - @pnpm/lockfile.types@1100.1.1
  - @pnpm/network.auth-header@1101.1.13
  - @pnpm/network.fetch@1100.1.15
  - @pnpm/network.web-auth@1101.6.0
  - @pnpm/releasing.exportable-manifest@1100.3.0
  - @pnpm/releasing.versioning@1100.3.0
  - @pnpm/resolving.npm-resolver@1104.1.1
  - @pnpm/workspace.projects-filter@1100.0.41
  - @pnpm/workspace.projects-graph@1100.0.36
  - @pnpm/workspace.task-scheduler@1100.0.1
  - @pnpm/workspace.workspace-manifest-writer@1100.1.3
