---
id: setup
title: pnpm setup
---

This command is used by the standalone installation scripts of pnpm. For instance, in [https://get.pnpm.io/install.sh].

Setup does the following actions:

* creates a home directory for the pnpm CLI
* adds the pnpm home directory to the `PATH` by updating the shell configuration file
* copies the pnpm executable to the pnpm home directory

Since v11.18.0, when running on GitHub Actions, `pnpm setup` also appends `PNPM_HOME` and the global bin directory to the GitHub Actions environment files (`GITHUB_ENV` and `GITHUB_PATH`), so later steps in the same job can run `pnpm add --global` and other global commands.

:::tip

After upgrading to pnpm v11, run `pnpm setup` to update your shell configuration. In v11, globally installed binaries are stored in a `bin` subdirectory of `PNPM_HOME`.

:::

[https://get.pnpm.io/install.sh]: https://get.pnpm.io/install.sh
