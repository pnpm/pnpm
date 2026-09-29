---
id: shim
title: "pnpm shim <cmd>"
---

Added in: v12.0.0-rc.6 (pnpm v12 only)

Manage project-aware command shims.

A shim created by this command has no global installation behind it. It exists only to let the project you are standing in decide what runs: `pnpm shim add yarn` links a `yarn` command that runs whatever version the current project pins, and provisions that version on demand. On a machine that has only pnpm, that is what makes `yarn` work inside a Yarn project.

Creating a shim is always deliberate — nothing writes one as a side effect of [`pnpm setup`](./setup.md) or an install, because a shim shadows whatever the rest of your `PATH` resolved before it.

## Commands

### add

```
pnpm shim add <pkg>...
```

Links a shim for every bin of each package into the global bin directory (`pnpm bin -g`), and records the package in the [`globalShims`](../settings/other.md#globalshims) setting so the shims actually dispatch.

```sh
pnpm shim add yarn
```

The [package managers pnpm provisions](../package-managers.md) — `npm`, `yarn`, `bun` — need no lookup: their bins are known up front, aliases included (`yarn` and `yarnpkg`, `npm` and `npx`). For any other package, the bins are read from the package's published manifest; a package that publishes no bin fails with `ERR_PNPM_SHIM_NO_BINS`.

A bin that is already in the global bin directory belongs to something else — a globally installed package, or another package's shim — so the command refuses with `ERR_PNPM_SHIM_BIN_CONFLICT` rather than taking a working command away.

Adding a shim while `globalShims: false` is set fails with `ERR_PNPM_SHIMS_DISABLED`: that setting turns every project-aware shim off, so the shim would sit on `PATH` doing nothing.

### rm

```
pnpm shim rm <pkg>...
```

Aliases: `remove`, `uninstall`

Removes the package's shims and the `globalShims` entry that `add` recorded for it.

### ls

```
pnpm shim ls
```

Alias: `list`

Lists every shim in the global bin directory with the policy governing it:

```text
yarn (auto): yarn, yarnpkg
```

## How a version is chosen

A shim resolves the project you are standing in, walking up from the current working directory:

* For a **package manager**, the project's [`packageManager`](../package_json.md) or [`devEngines.packageManager`](../package_json.md#devenginespackagemanager) pin decides the version, and pnpm provisions it. The pin outranks a copy of that package manager installed globally, because it is the project's own statement of what installs it.
* For **any other package**, the project's `node_modules/.bin/<name>` is used.

If nothing in the project provides the command, the globally installed version runs — a shim never makes a command fail merely because dispatch did not apply.

The [trust rules](../global-packages.md#trust) of project-aware global bins apply here too: an npm-published package manager is verified against npm's signature for its exact version and switches without asking, while Bun and Yarn 6 arrive as checksum-pinned platform archives and go through the confirmation prompt. That answer is remembered per project *and* per binary, so moving the project's pin to another version asks again.

## The shim is a real executable

Since v12.3.0, every project-aware global command pnpm writes is a native
executable on every platform: the shims `pnpm shim add` creates, and the `node`,
`deno`, and `bun` commands. On Windows that means `<name>.exe` instead of the
`.cmd` and `.ps1` pair earlier pnpm 12 releases wrote.

The practical difference is that a shell no longer sits between you and the
command, so environment variables whose names are not valid shell identifiers
survive the hop. Shims written by an earlier pnpm 12 are replaced on the next
global install or `pnpm self-update`.

## Related

* [Other package managers](../package-managers.md)
* [Project-aware global bins](../global-packages.md#project-aware-global-bins)
* [`globalShims`](../settings/other.md#globalshims)
