## 1101.0.10

### Patch Changes

- Ensure path-scoped registry prefix matching enforces a path segment boundary during unpublish [pnpm/tasks#94](https://github.com/pnpm/tasks/issues/94).

- `pnpm unpublish <pkg>@<version>` now deletes the tarball under the registry's path when the registry is served under one, such as Gitea's npm registry. It used to send the delete to the host root and report success without removing the version [#16568](https://github.com/pnpm/pnpm/issues/16568).

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.32
  - @pnpm/config.reader@1102.3.4
  - @pnpm/error@1100.2.2
  - @pnpm/network.auth-header@1101.1.16
  - @pnpm/network.fetch@1100.1.22
  - @pnpm/network.web-auth@1101.6.3
  - @pnpm/registry-access.client@1100.1.24
