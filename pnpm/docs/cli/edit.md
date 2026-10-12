---
id: edit
title: "pnpm edit <pkg>"
---

Open an installed dependency's folder in a text editor. After the editor exits successfully, pnpm rebuilds only the edited dependency.

```sh
pnpm edit is-positive
pnpm edit express/safe-buffer
pnpm edit @scope/package --editor "code --wait"
```

Edits apply to the installed files in your project. pnpm breaks their hard links before opening the editor to protect other projects that share the store. Build results from edited files are not saved to shared caches. Use [`pnpm patch`](patch.md) to create a patch that survives reinstalls.

`pnpm edit` does not support a shared virtual store (`enableGlobalVirtualStore: true`). Use `pnpm patch` to change a dependency for your project in this mode.

## --editor <editor>

Choose the editor command. You can include arguments, such as `code --wait`.

Set `editor` in `pnpm-workspace.yaml` or the global configuration file to choose a default. You can also set `PNPM_CONFIG_EDITOR`.

Without this option, pnpm uses the configured editor, then `EDITOR`, then `VISUAL`, then `notepad` on Windows or `vi` on other systems. Empty or whitespace-only `EDITOR` and `VISUAL` values are ignored.

A bare editor command must resolve to an executable outside your project. To select an editor inside the project explicitly, provide its path with `--editor`.
