---
id: completion
title: Command line tab-completion
---

:::info

Completion for pnpm v9 is incompatible with completion for older pnpm versions.
If you have already installed pnpm completion for a version older than v9, you must uninstall it first to ensure that completion for v9 works properly.
You can do this by removing the section of code that contains `__tabtab` in your dot files.

:::

Unlike other popular package managers, which usually require plugins, pnpm
supports command line tab-completion for Bash, Zsh, Fish, and similar shells.

To setup autocompletion for Bash, run:

```text
pnpm completion bash > ~/completion-for-pnpm.bash
echo 'source ~/completion-for-pnpm.bash' >> ~/.bashrc
```

To setup autocompletion for Fish, run:

```text
pnpm completion fish > ~/.config/fish/completions/pnpm.fish
```

## g-plane/pnpm-shell-completion

[pnpm-shell-completion] is a shell plugin maintained by Pig Fang on Github.

Features:

- Provide completion for `pnpm --filter <package>`.
- Provide completion for `pnpm remove` command, even in workspace's packages (by specifying `--filter` option).
- Provide completion for scripts in `package.json`.

[pnpm-shell-completion]: https://github.com/g-plane/pnpm-shell-completion
