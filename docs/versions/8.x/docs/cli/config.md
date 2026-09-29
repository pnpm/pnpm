---
id: config
title: pnpm config
---

Aliases: `c`

Manage the configuration files.

The configuration files are in `INI` format.

The local configuration file is located in the root of the project and is named `.npmrc`.

The global configuration file is located at one of the following locations:

* If the **$XDG_CONFIG_HOME** env variable is set, then **$XDG_CONFIG_HOME/pnpm/rc**
* On Windows: **~/AppData/Local/pnpm/config/rc**
* On macOS: **~/Library/Preferences/pnpm/rc**
* On Linux: **~/.config/pnpm/rc**

## Commands

### set &lt;key> &lt;value>

Set the config key to the value provided.

### get &lt;key>

Print the config value for the provided key.

### delete &lt;key>

Remove the config key from the config file.

### list

Show all the config settings.

## Options

### --global, -g

Set the configuration in the global config file.

### --location

When set to `project`, the `.npmrc` file at the nearest `package.json` will be used.

When set to `global`, the performance is the same as setting the `--global` option.

### --json

Show all the config settings in JSON format.
