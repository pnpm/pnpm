---
id: completion
title: Command line tab-completion
---

Added in: v4.9.0

Unlike other popular package managers, which usually require plugins, pnpm
supports command line tab-completion for Bash, Zsh, Fish, and similar shells.

To setup autocompletion, run:

```text
pnpm install-completion
```

The CLI will ask for which shell to generate the autocompletion script.
Alternatively, the target shell may be specified in the command line (since
v5.15.0):

```text
pnpm install-completion zsh
```

To see examples of completion, read [this article].

[this article]: https://medium.com/pnpm/pnpm-v4-9-comes-with-command-completion-a411715260b4
