---
id: setup
title: pnpm setup
---

This command is used by the standalone installation scripts of pnpm. For instance, in [https://get.pnpm.io/install.sh].

Setup does the following actions:

* creates a home directory for the pnpm CLI
* adds the pnpm home directory to the `PATH` by updating the shell configuration file
* when pnpm runs as a standalone executable, installs it as a global package and writes the `pn`, `pnpx`, and `pnx` scripts to `$PNPM_HOME/bin`

If `pnpm setup` cannot tell which shell configuration file to update, for example
in a container where `SHELL` is not set, it still installs pnpm. It then prints the
`PNPM_HOME` and `PATH` lines to add to your shell configuration yourself.

On Windows, if the registry environment values cannot be read or written,
`pnpm setup` still installs pnpm. It prints commands to set `PNPM_HOME` and
`PATH` in the current PowerShell session. Add these commands to your PowerShell
profile to use pnpm in future sessions without registry access.

Since v11.18.0, when running on GitHub Actions, `pnpm setup` also appends `PNPM_HOME` and the global bin directory to the GitHub Actions environment files (`GITHUB_ENV` and `GITHUB_PATH`), so later steps in the same job can run `pnpm add --global` and other global commands.

:::tip

After upgrading to pnpm v11, run `pnpm setup` to update your shell configuration. In v11, globally installed binaries are stored in a `bin` subdirectory of `PNPM_HOME`.

:::

[https://get.pnpm.io/install.sh]: https://get.pnpm.io/install.sh
