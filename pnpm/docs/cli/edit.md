---
id: edit
title: "pnpm edit <pkg>"
---

Open an installed dependency's folder in a text editor. After the editor exits successfully, pnpm rebuilds the package.

```sh
pnpm edit is-positive
pnpm edit express/safe-buffer
pnpm edit @scope/package --editor "code --wait"
```

Edits apply to the installed files in your project. pnpm breaks their hard links before opening the editor to protect other projects that share the store. Use [`pnpm patch`](patch.md) to create a patch that survives reinstalls.

## --editor <editor>

Choose the editor command. You can include arguments, such as `code --wait`.

Without this option, pnpm uses the configured editor, then `EDITOR`, then `VISUAL`, then `notepad` on Windows or `vi` on other systems.

A bare editor command must resolve to an executable outside your project. To select an editor inside the project explicitly, provide its path with `--editor`.
