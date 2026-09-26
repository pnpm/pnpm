## 0.1.0-alpha.13

### Patch Changes

- Requests to a registry or tarball server whose TLS certificate fails verification now fail at once. Such requests were retried for more than a minute without any output [#9134](https://github.com/pnpm/pnpm/issues/9134).

- On macOS, `pnpm` now uses its bundled certificate roots when macOS cannot create an SSL policy for a registry connection. It crashed on the first registry request in that case [pnpm/pnpm#14461](https://github.com/pnpm/pnpm/issues/14461).

- Installing through a pnpr server now installs a project's peer dependencies when `autoInstallPeers` is enabled. A project that declared only peer dependencies failed with `ERR_PNPM_OUTDATED_LOCKFILE` or skipped its peers [#14833](https://github.com/pnpm/pnpm/issues/14833).

- On macOS, pnpr now builds its package index faster when it starts over a storage directory that has no index yet.

- The pnpr config file now supports `${VAR?}` placeholders. They expand to the value of `VAR`, or to an empty string without a warning when `VAR` is unset [#14404](https://github.com/pnpm/pnpm/issues/14404).
