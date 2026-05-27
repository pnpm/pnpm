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

# Registry & Authentication Settings

| Done | Field              | Notes |
|------|--------------------|-------|
| ✅    | registry           |       |
|      | <URL>:_authToken   |       |
|      | <URL>:_tokenHelper |       |

# Request Settings

**Not implemented**

# Peer Dependency Settings

| Done | Field                             | Notes |
|------|-----------------------------------|-------|
| ✅    | auto_install_peers                |       |
|      | dedupe_peer_dependents            |       |
|      | strict_peer_dependencies          |       |
|      | resolve_peers_from_workspace_root |       |
