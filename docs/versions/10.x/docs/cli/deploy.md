---
id: deploy
title: "pnpm deploy"
---

Deploy a package from a workspace. During deployment, the files of the deployed package are copied to the target directory. All dependencies of the deployed package, including dependencies from the workspace, are installed inside an isolated `node_modules` directory at the target directory. The target directory will contain a portable package that can be copied to a server and executed without additional steps.

:::note

By default, the deploy command only works with workspaces that have the `inject-workspace-packages` setting set to `true`. If you want to use deploy without "injected dependencies", use the `--legacy` flag or set `force-legacy-deploy` to `true`.

:::

:::note

When the [`enableGlobalVirtualStore`](../settings.md#enableglobalvirtualstore) option is set, `pnpm deploy` ignores it and always creates a localized virtual store within the deploy directory. This keeps the deploy directory self-contained and portable.

:::

Usage:

```
pnpm --filter=<deployed project name> deploy <target directory>
```

In case you build your project before deployment, also use the `--prod` option to skip `devDependencies` installation.

```
pnpm --filter=<deployed project name> --prod deploy <target directory>
```

Usage in a docker image. After building everything in your monorepo, do this in a second image that uses your monorepo base image as a build context or in an additional build stage:

```Dockerfile
# syntax=docker/dockerfile:1.4

FROM workspace as pruned
RUN pnpm --filter <your package name> --prod deploy pruned

FROM node:18-alpine
WORKDIR /app

ENV NODE_ENV=production

COPY --from=pruned /app/pruned .

ENTRYPOINT ["node", "index.js"]
```

## Options

### --dev, -D

Only `devDependencies` are installed.

### --no-optional

`optionalDependencies` are not installed.

### --prod, -P

Packages in `devDependencies` won't be installed.

### --filter &lt;package_selector\>

[Read more about filtering.](../filtering.md)

### --legacy

Force legacy deploy implementation.

By default, `pnpm deploy` will try creating a dedicated lockfile from a shared lockfile for deployment. The `--legacy` flag disables this behavior and also allows using the deploy command without the `inject-workspace-packages=true` setting.

## Files included in the deployed project

By default, all the files of the project are copied during deployment but this can be modified in _one_ of the following ways which are resolved in order:

1. The project's `package.json` may contain a "files" field to list the files and directories that should be copied.
2. If there is an `.npmignore` file in the application directory then any files listed here are ignored.
3. If there is a `.gitignore` file in the application directory then any files listed here are ignored.

## Configuration

### forceLegacyDeploy

* Default: **false**
* Type: **Boolean**

By default, `pnpm deploy` will try creating a dedicated lockfile from a shared lockfile for deployment. If this setting is set to `true`, the legacy `deploy` behavior will be used.
