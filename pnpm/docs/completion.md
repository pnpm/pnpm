---
id: completion
title: Command line tab-completion
---

Unlike other popular package managers, which usually require plugins, pnpm
supports command line tab-completion for Bash, Fish, PowerShell, and Zsh.

`pnpm completion <shell>` prints completion code to standard output. Supported
shell names are `bash`, `fish`, `pwsh`, and `zsh`.

To setup autocompletion for Bash, run:

```sh
pnpm completion bash > ~/completion-for-pnpm.bash
echo 'source ~/completion-for-pnpm.bash' >> ~/.bashrc
```

To setup autocompletion for Fish, run:

```sh
pnpm completion fish > ~/.config/fish/completions/pnpm.fish
```

To setup autocompletion for Zsh, run:

```sh
pnpm completion zsh > ~/completion-for-pnpm.zsh
echo 'source ~/completion-for-pnpm.zsh' >> ~/.zshrc
```

To setup autocompletion for PowerShell, run:

```powershell
pnpm completion pwsh >> $PROFILE
```

Any other shell name fails with `ERR_PNPM_UNSUPPORTED_SHELL`.

Since v12.4.0, the generated completions cover the [`pn` alias](./pnpm-cli.md#short-aliases) as well, in Bash, Fish, PowerShell, and Zsh.

## g-plane/pnpm-shell-completion

[pnpm-shell-completion] is a shell plugin maintained by Pig Fang on GitHub.

Features:

- Provide completion for `pnpm remove` command, even in workspace's packages (by specifying `--filter` option).

[pnpm-shell-completion]: https://github.com/g-plane/pnpm-shell-completion
