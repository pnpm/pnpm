---
id: uninstall
title: Uninstalling pnpm
---

## Removing the globally installed packages

Before removing the pnpm CLI, it might make sense to remove all global packages that were installed by pnpm.

To list all the global packages, run `pnpm ls -g`. There are two ways to remove the global packages:

1. Run `pnpm rm -g <pkg>...` with each global package listed.
2. Run `pnpm root -g` to find the location of the global directory and remove it manually.

## Removing the pnpm CLI

If you used the standalone script to install pnpm (or npx), then you should be able to uninstall the pnpm CLI using:

```
pnpm rm -g pnpm
```

You might also want to clean the `PNPM_HOME` env variable in your shell configuration file (`$HOME/.bashrc`, `$HOME/.zshrc` or `$HOME/.config/fish/config.fish`).

If you used npm to install pnpm, then you should use npm to uninstall pnpm:

```
npm rm -g pnpm
```

## Removing the global content-addressable store

If you used pnpm only in the primary disk, then you will have a global store in the home directory. So just remove it via:

```
rm -rf ~/.pnpm-store
```

If you used pnpm in non-primary disks, then the store is in the root of that disk. For instance, if you used pnpm on disk `D:` on Windows, remove the store from `D:\.pnpm-store`.

## Removing the state file

pnpm also saves some state in `~/.pnpm-state.json`. You may remove this file.
