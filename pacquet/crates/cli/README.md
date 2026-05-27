# Options

[pnpm documentation](https://pnpm.io/pnpm-cli#options)

| Done | Command                 | Notes |
| ---- | ----------------------- | ----- |
| ✅   | -C <path>, --dir <path> |       |
|      | -w, --workspace-root    |       |

# Manage dependencies

## `pacquet add <pkg>`

[pnpm documentation](https://pnpm.io/cli/add)

- [~] Install from npm registry
  - Install with tags are not supported. Example: `pacquet add fastify@latest`
- [ ] Install from the workspace
- [ ] Install from local file system
- [ ] Install from remote tarball
- [ ] Install from Git repository

| Done | Command                       | Notes |
| ---- | ----------------------------- | ----- |
| ✅   | --save-prod                   |       |
| ✅   | --save-dev                    |       |
| ✅   | --save-optional               |       |
| ✅   | --save-exact                  |       |
| ✅   | --save-peer                   |       |
|      | --ignore-workspace-root-check |       |
|      | --global                      |       |
|      | --workspace                   |       |
|      | --filter <package_selector>   |       |

## `pacquet install`

[pnpm documentation](https://pnpm.io/cli/install)

| Done | Command                     | Notes |
| ---- | --------------------------- | ----- |
|      | --force                     |       |
| ✅   | --offline                   | Frozen-install only: refuses network fetches; errors with `ERR_PACQUET_NO_OFFLINE_TARBALL` when a snapshot isn't cached. Stage 2 resolver will additionally gate metadata fetches like upstream's [`pickPackage`](https://github.com/pnpm/pnpm/blob/94240bc046/resolving/npm-resolver/src/pickPackage.ts). |
| ✅   | --prefer-offline            | No-op on frozen-install (warm prefetch already prefers the local store). Reserved for Stage 2's resolver. |
|      | --prod                      |       |
| ✅   | --dev                       |       |
| ✅   | --no-optional               |       |
|      | --lockfile-only             |       |
|      | --fix-lockfile              |       |
|      | --frozen-lockfile           |       |
|      | --reporter=<name>           |       |
|      | --use-store-server          |       |
|      | --shamefully-hoist          |       |
|      | --ignore-scripts            |       |
|      | --filter <package_selector> |       |
|      | --resolution-only           |       |

# Run scripts

## `pacquet run`

[pnpm documentation](https://pnpm.io/cli/run)

| Done | Command                      | Notes |
| ---- | ---------------------------- | ----- |
|      | script-shell                 |       |
|      | shell-emulator               |       |
|      | --recursive                  |       |
| ✅   | --if-present                 |       |
|      | --parallel                   |       |
|      | --stream                     |       |
|      | --aggregate-output           |       |
|      | enable-pre-post-scripts      |       |
|      | --resume-from <package_name> |       |
|      | --report-summary             |       |
|      | --filter <package_selector>  |       |

## `pacquet test`

[pnpm documentation](https://pnpm.io/cli/test)

## `pacquet start`

[pnpm documentation](https://pnpm.io/cli/start)

# Misc.

## `pacquet store`

[pnpm documentation](https://pnpm.io/cli/store)

| Done | Command | Notes                                                     |
| ---- | ------- | --------------------------------------------------------- |
|      | status  |                                                           |
|      | add     |                                                           |
| ~    | prune   | Currently prune removes all packages inside the directory |
| ✅   | path    |                                                           |

## `pacquet init`

[pnpm documentation](https://pnpm.io/cli/init)
