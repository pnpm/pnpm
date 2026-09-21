For more information, read [pnpm docs about .npmrc](https://pnpm.io/npmrc)

# Dependency Hoisting Settings

| Done | Field                | Notes |
|------|----------------------|-------|
|      | hoist                |       |
|      | hoist_pattern        |       |
|      | public_hoist_pattern |       |
|      | shamefully_hoist     |       |

# Node-Modules Settings

| Done | Field                 | Notes                               |
|------|-----------------------|-------------------------------------|
| ✅    | store_dir             |                                     |
| ✅    | modules_dir           |                                     |
|      | node_linker           |                                     |
|      | symlink               |                                     |
| ✅    | virtual_store_dir     |                                     |
| ~    | package_import_method | Only "auto" is implemented for now. |
|      | modules_cache_max_age |                                     |

# Lockfile Settings

| Done | Attribute                    | Notes |
|------|------------------------------|-------|
|      | lockfile                     |       |
|      | prefer_frozen_lockfile       |       |
|      | lockfile_include_tarball_url |       |
| ✅    | exclude_links_from_lockfile  |       |

# Registry & Authentication Settings

| Done | Field              | Notes |
|------|--------------------|-------|
| ✅    | registry           |       |
|      | <URL>:_authToken   |       |
| ✅    | <URL>:tokenHelper  | Trusted sources only; executed lazily on lookup. |

# Request Settings

**Not implemented**

# Peer Dependency Settings

| Done | Field                             | Notes |
|------|-----------------------------------|-------|
| ✅    | auto_install_peers                |       |
|      | dedupe_peer_dependents            |       |
|      | strict_peer_dependencies          |       |
|      | resolve_peers_from_workspace_root |       |

## Automatic deduplication

Set `autoDedupe: true` in `pnpm-workspace.yaml` to consolidate compatible
versions during dependency resolution. It defaults to `false`.
`pnpm install --auto-dedupe` and `pnpm add --auto-dedupe` enable it for one
command; `--no-auto-dedupe` disables the workspace setting.

Automatic deduplication retains existing versions as preferences and respects
consumer ranges, overrides, peer dependencies, and resolution policies. It does
not add overrides, save convergence pins, or change the lockfile format.
`--frozen-lockfile` continues to install the recorded graph without deduplication.

An unchanged materialized installation retains the repeat-install fast path.
When resolution is needed, branches containing multiple versions of a package
are reopened while unrelated subtrees remain reusable. Changes that invalidate
lockfile reuse conservatively reconsider all locked package names.
Newly discovered duplicate versions feed further resolution rounds using the
same metadata cache within one installation. Lifecycle scripts run after the
final graph is resolved.

This setting currently requires local dependency resolution. A non-frozen
install using `pnprServer` reports an error when `autoDedupe` is enabled.
