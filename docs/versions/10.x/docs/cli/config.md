---
id: config
title: pnpm config
---

Aliases: `c`

Manage the configuration files.

The configuration files are in `INI` (the global) and `YAML` (the local) formats.

The local configuration file is located in the root of the project and is named `pnpm-workspace.yaml`.

The global configuration file is located at one of the following locations:

* If the **$XDG_CONFIG_HOME** env variable is set, then **$XDG_CONFIG_HOME/pnpm/rc**
* On Windows: **~/AppData/Local/pnpm/config/rc**
* On macOS: **~/Library/Preferences/pnpm/rc**
* On Linux: **~/.config/pnpm/rc**

You can also retrieve the path to your global config file by running (added in v10.21.0):

```sh
pnpm config get globalconfig
```

## Commands

### set &lt;key> &lt;value>

Set the config key to the value provided.

Without the `--json` flag, it parses the value as plain string:

```sh
pnpm config set --location=project nodeVersion 22.0.0
```

With the `--json` flag, it parses the value as JSON:

```sh
pnpm config set --location=project --json nodeVersion '"22.0.0"'
```

The `--json` flag also allows `pnpm config set` to create arrays and objects:

```sh
pnpm config set --location=project --json onlyBuiltDependencies '["react", "react-dom"]'
pnpm config set --location=project --json catalog '{ "react": "19" }'
```

The `set` command does not accept a property path.

### get &lt;key>

Print the config value for the provided key.

The `key` can be a simple key:

```sh
pnpm config get nodeVersion
pnpm config get --json nodeVersion
pnpm config get --json packageExtensions
pnpm config get --json onlyBuiltDependencies
pnpm config get --json catalog
```

It can also be a property path:

```sh
pnpm config get 'packageExtensions["@babel/parser"].peerDependencies["@babel/types"]'
pnpm config get --json 'packageExtensions["@babel/parser"].peerDependencies["@babel/types"]'
pnpm config get 'onlyBuiltDependencies[0]'
pnpm config get --json 'onlyBuiltDependencies[0]'
pnpm config get catalog.react
pnpm config get --json catalog.react
```

The syntax of the property path emulates JavaScript property paths.

### delete &lt;key>

Remove the config key from the config file.

### list

Show all the config settings.

## Options

### --global, -g

Set the configuration in the global config file.

### --location

By default, `--location` is set to `global`.

When set to `project`, the `.npmrc` file at the nearest `package.json` will be used. If no `.npmrc` file is present in the directory, the setting will be written to a `pnpm-workspace.yaml` file.

When set to `global`, the performance is the same as setting the `--global` option.

### --json

Make `get` and `list` show all the config settings in JSON format and make `set` parse the value as JSON.
