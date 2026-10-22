---
id: configuring
title: Configuring
---

pnpm settings are divided into two categories:

- **Authentication and certificate settings** are stored in INI files. These contain sensitive credentials and should not be committed to your repository. See [Authentication Settings](./npmrc.md#auth-file-locations) for details.
- **All other settings** are stored in YAML files: the project `pnpm-workspace.yaml` and the global `config.yaml`. Some settings are project-only and are ignored in the global `config.yaml` (see [Global configuration](#global-configuration)).

pnpm does not read settings from the `pnpm` field of `package.json`. A setting found there is ignored with a warning. Define it in `pnpm-workspace.yaml`.

## Local project configuration

Project-level settings go in `pnpm-workspace.yaml`:

```yaml title="pnpm-workspace.yaml"
nodeVersion: "22.22.3"
saveExact: true
```

`pnpm-workspace.yaml` can delegate settings to other files and to config dependencies. See [Splitting Settings Across Files](./config-includes.md).

## Global configuration

The global YAML config file (`config.yaml`) is located at one of the following paths:

* If the **$XDG_CONFIG_HOME** env variable is set, then **$XDG_CONFIG_HOME/pnpm/config.yaml**
* On Windows: **%LOCALAPPDATA%\pnpm\config\config.yaml**
* On macOS: **~/Library/Preferences/pnpm/config.yaml**
* On Linux: **~/.config/pnpm/config.yaml**

Not every setting may be set in the global `config.yaml`. Settings that apply to the whole machine, such as `storeDir`, `cacheDir`, `packageImportMethod`, `minimumReleaseAge`, and network settings, may be set there. Settings that define the structure of a project's `node_modules` or workspace, such as `nodeLinker`, `hoistPattern`, `publicHoistPattern`, `shamefullyHoist`, or `linkWorkspacePackages`, are project-only: they have to be set in `pnpm-workspace.yaml`, so that the project installs the same way on every machine. Registry routes may be set globally, but the server descriptions `serverType` and `supportsTimeField` are read only from `pnpm-workspace.yaml` (see [Registries](./registries.md#where-the-setting-may-live)).

`pnpm config set --global` refuses project-only settings with `ERR_PNPM_CONFIG_SET_UNSUPPORTED_YAML_CONFIG_KEY`. If they are written to `config.yaml` by hand, pnpm prints a warning that names them and ignores them.

To share project-only settings across several projects, publish them as a [config dependency](./config-dependencies.md) and add it to each project.

`pnpm login` saves registry tokens in this `config.yaml`. Other global auth and certificate settings go in `auth.ini` in the same directory. See [Auth file locations](./npmrc.md#auth-file-locations).

## Environment variables

Environment variables whose names start with `pnpm_config_` (or `PNPM_CONFIG_`) are loaded into configuration. These override settings from `pnpm-workspace.yaml` but not CLI arguments.

:::warning

pnpm does not read `npm_config_*` environment variables. Use `pnpm_config_*` environment variables instead (e.g., `pnpm_config_registry` instead of `npm_config_registry`). The exceptions are:

* `NPM_CONFIG_WORKSPACE_DIR`, which sets the workspace directory.
* `npm_config_userconfig`, which points to a user `.npmrc` for auth settings.
* URL-scoped credentials such as `npm_config_//registry.example.com/:_authToken`.

:::

For example:

```sh
pnpm_config_save_exact=true pnpm add foo
```

If you need pnpm to work across multiple hard drives or filesystems,
read [the FAQ].

See the [`config` command] for more information on managing configuration.

[the FAQ]: ./faq.md#does-pnpm-work-across-multiple-drives-or-filesystems
[`config` command]: ./cli/config.md
