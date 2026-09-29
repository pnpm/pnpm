---
id: scripts
title: Scripts
---

How pnpm handles the `scripts` field of `package.json`.

## Hidden Scripts

Added in: v11.0.0

Scripts with names starting with `.` are hidden. They cannot be run directly via `pnpm run` and are omitted from the `pnpm run` listing. Hidden scripts can only be called from other scripts.

```json
{
  "scripts": {
    ".helper": "echo 'I am hidden'",
    "build": "pnpm run .helper && tsc"
  }
}
```

In this example, `pnpm run .helper` would fail, but `pnpm run build` would succeed because `.helper` is called from another script.

## Environment Variables

pnpm sets the following environment variables during lifecycle script execution:

- `npm_package_name` — the package name
- `npm_package_version` — the package version
- `npm_lifecycle_event` — the name of the running script (e.g., `postinstall`)

:::note

Since v11, pnpm no longer populates `npm_config_*` environment variables from the pnpm configuration. Only the well-known `npm_*` variables above are set, matching Yarn's behavior.

:::

## Built-in Command and Script Name Conflicts

Added in: v11.0.0

The following built-in commands prefer user scripts: `clean`, `setup`, `deploy`, and `rebuild`. If your `package.json` defines a script with one of these names, `pnpm <name>` will execute the script instead of the built-in command.

To force the built-in command, use [`pnpm pm <name>`](./cli/pm.md).

## Lifecycle Scripts

### `pnpm:devPreinstall`

Runs when `pnpm install` runs in the project itself, including in CI. It does not run when the package is installed as a dependency of another project.

Runs before any dependency is installed.

This script is executed only when set in the root project's `package.json`.

Since v11.28.0, installs that skip `devDependencies`, such as `pnpm install --prod`, do not run it.

### `preinstall` of the root project

Since v11.28.0, the root project's `preinstall` script runs before dependencies are resolved and linked. A guard such as `npx only-allow pnpm` can therefore stop the install before pnpm populates `node_modules`.

### `prepare`

The root project's `prepare` script runs after `pnpm install`. It is skipped, since v11.28.0, by installs that skip `devDependencies` (such as `pnpm install --prod`), by `pnpm install` given package arguments, and for the project that [`pnpm deploy`](./cli/deploy.md) deploys.

### Uninstall scripts

Added in: v11.28.0

[`pnpm remove`](./cli/remove.md) runs the project's own `preuninstall`, `uninstall`, and `postuninstall` scripts. `preuninstall` and `uninstall` run before dependencies are unlinked, and a failure in either aborts the removal. `postuninstall` runs after unlinking completes. The [`ignoreScripts`](./settings/build.md#ignorescripts) setting and `--lockfile-only` skip all three.
