---
id: edit
title: "pnpm edit <pkg>"
---

Opens an installed package's folder in the default text editor. After the editor exits successfully, pnpm rebuilds only the edited dependency.

```sh
pnpm edit is-positive
pnpm edit express/safe-buffer
pnpm edit @scope/package --editor "code --wait"
```

Edits apply to the installed files in your project. pnpm breaks their hard links before opening the editor to protect other projects that share the store. Build results from edited files are not saved to shared caches. Use [`pnpm patch`](patch.md) to create a patch that survives reinstalls.

`pnpm edit` does not support a shared virtual store (`enableGlobalVirtualStore: true`). Use `pnpm patch` to change a dependency for your project in this mode.

## --editor <editor>

Choose the editor command. You can include arguments, such as `code --wait`.

Set `editor` in the global configuration file or in `PNPM_CONFIG_EDITOR` to choose a default. `pnpm-workspace.yaml` cannot set it, because a repository must not choose the program pnpm runs.

Without this option, pnpm uses the configured editor, then `EDITOR`, then `VISUAL`, then `notepad` on Windows or `vi` on other systems. Empty or whitespace-only `EDITOR` and `VISUAL` values are ignored.

A bare editor command must resolve to an executable outside your project. To select an editor inside the project explicitly, provide its path with `--editor`.
