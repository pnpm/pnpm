---
id: setup
title: pnpm setup
---

Installs the running pnpm into `PNPM_HOME` and adds it to `PATH`. The standalone installation scripts of pnpm run this command, for instance [https://get.pnpm.io/install.sh].

```sh
pnpm setup [--force]
```

Setup does the following actions:

* creates a home directory for the pnpm CLI
* adds the pnpm home directory to the `PATH` by updating the shell configuration file
* installs the running pnpm executable as a [global package](../global-packages.md)
* writes the `pn`, `pnpx`, and `pnx` scripts to `$PNPM_HOME/bin`, the directory of globally installed binaries

To make a runtime command project-aware after setup, see [`pnpm shim`](./shim.md).

If `pnpm setup` cannot tell which shell configuration file to update, for example
in a container where `SHELL` is not set, it still installs pnpm. It then prints the
`PNPM_HOME` and `PATH` lines to add to your shell configuration yourself.

On Windows, if the registry environment values cannot be read or written,
`pnpm setup` still installs pnpm. It prints commands to set `PNPM_HOME` and
`PATH` in the current PowerShell session. Add these commands to your PowerShell
profile to use pnpm in future sessions without registry access.

When running on GitHub Actions, `pnpm setup` also appends `PNPM_HOME` and the global bin directory to the GitHub Actions environment files (`GITHUB_ENV` and `GITHUB_PATH`), so later steps in the same job can run `pnpm add --global` and other global commands.

## Options

### --force, -f

Override the `PNPM_HOME` environment variable if it already exists.

[https://get.pnpm.io/install.sh]: https://get.pnpm.io/install.sh
