---
id: package-managers
title: Other package managers
---

Added in: v12.0.0-rc.6 (pnpm v12 only)

pnpm installs the other package managers, not just itself: **npm**, **Yarn Classic**, **Yarn Berry**, **Yarn 6** (`yarnpkg/zpm`) and **Bun**. The npm-published ones are resolved and fetched through the trusted package-manager registries, and verified against npm's signature for the exact version before that version is executed — the same standard that lets pnpm switch its own version without asking. Yarn 6 and Bun ship as platform archives from their own projects instead, pinned by a publisher checksum.

A JavaScript package manager on a machine without Node.js gets a managed LTS runtime to run on, so none of this requires a Node.js installation of your own.

Three things use it:

* a **git-hosted dependency** is prepared with the package manager it asks for — see [Preparing a git-hosted dependency](#preparing-a-git-hosted-dependency);
* [`pnpm dlx`](./cli/pnx.md#running-a-package-manager-or-a-runtime) (`pnx`) runs one for a single command;
* [`pnpm shim add`](./cli/shim.md) links a command that runs whatever version the current project pins.

## Where each one comes from

| Package manager | Provided by |
|---|---|
| npm | the `npm` package on the registry |
| Yarn Classic (`<2`) | the `yarn` package on the registry |
| Yarn Berry (`2` – `5`) | the `@yarnpkg/cli-dist` package on the registry |
| Yarn 6 | `yarnpkg/zpm` release archives |
| Bun | `oven-sh/bun` release archives |

The registry-published ones go through the same resolve, verify and install pipeline as pnpm's own engine. The ones that ship as platform archives are pinned by a publisher checksum, like the [managed runtimes](./cli/runtime.md).

Each package manager resolves into its own environment lockfile under the pnpm home, so one package manager's pins never rewrite another's.

## Declaring the package manager of a project

Naming a package manager in `pnpm add` records which one the project uses instead of installing the npm package that shares the name:

```sh
pnpm add yarn@4
```

The declaration goes where the package manager reads it:

* **Yarn** is started from a project pin through the `packageManager` field, which accepts only an exact version. So the requested line is resolved before it is written: `pnpm add yarn@4` records `"packageManager": "yarn@4.18.0"`. On the Yarn Classic line (`<2`), where the pinned artifact is the npm tarball, the value also carries that tarball's `+sha512.…` integrity — `"packageManager": "yarn@1.22.22+sha512.…"`.
* **Every other package manager** is recorded in [`devEngines.packageManager`](./package_json.md#devenginespackagemanager), which holds a range.

Only one of the two fields is ever left behind: they declare the same thing, and a project whose two declarations disagree is one the version switchers refuse to run.

```json title="package.json"
{
  "devEngines": {
    "packageManager": {
      "name": "npm",
      "version": "^11.0.0"
    }
  }
}
```

Two things are deliberately not covered by this:

* **pnpm itself.** Changing pnpm's own pin makes the next command switch the running CLI, which is [`pnpm self-update`](./cli/self-update.md)'s job to do deliberately rather than an `add`'s to do as a side effect.
* **A filtered selection.** Which package manager a project uses is that project's own declaration, so `pnpm add yarn --filter …` fails with `ERR_PNPM_PACKAGE_MANAGER_IN_SELECTION`. Run the command in the project itself.

A specifier that locates a *package* rather than asking for a released version still installs what it names, as an ordinary dependency:

```sh
pnpm add yarn@npm:yarn@1.22.22
pnpm add yarn@yarnpkg/berry
```

The resolved version of a package manager other than pnpm is not written to `pnpm-lock.yaml`: it is pinned in an environment lockfile of its own under the pnpm home. Only pnpm's own pin is recorded in the project's lockfile, under `packageManagerDependencies`.

### Runtimes

`pnpm add` follows the same rule for the runtimes: naming `node` or `deno` records it under `engines.runtime`, the way the explicit `node@runtime:22` spelling already did.

`bun` is both a runtime and a package manager, and the package manager answers first: `pnpm add bun` declares it as the project's package manager. To add it as a runtime, ask for one — `pnpm add bun@runtime:1.3.0`.

## Installing one globally

```sh
pnpm add -g yarn
```

installs the current Yarn line — not the Classic-only `yarn` package on npm — and `pnpm add -g node@22` / `pnpm add -g deno@2` install that Node.js or Deno release rather than a wrapper package that downloads one.

A globally installed package manager also follows a project's pin, the way a globally installed Node.js already follows [`devEngines.runtime`](./package_json.md#devenginesruntime): the pinned version runs where a project pins one, and the globally installed copy is the fallback everywhere else. pnpm records this by adding a [`globalShims`](./settings/other.md#globalshims) entry for the package manager, unless you already decided about it — an explicit entry, including `false`, is left as you set it.

See [Project-aware global bins](./global-packages.md#project-aware-global-bins) for how a version is chosen and when pnpm asks for confirmation.

## Running one for a single command

```sh
pnx yarn@4 install
pnx npm@11 ci
pnx bun@1.3.0 install
```

See [Running a package manager or a runtime](./cli/pnx.md#running-a-package-manager-or-a-runtime).

## Preparing a git-hosted dependency

A [git-hosted dependency](./package-sources.md#git-repository) that has to be built before it can be installed is prepared with the package manager it asks for, rather than with whatever the host happens to have:

* its `packageManager` / `devEngines.packageManager` pin is honored;
* failing a pin, the lockfile it ships names the package manager — and a `yarn.lock` written by Yarn Classic is no longer installed by Yarn Berry. Berry stamps `__metadata:` into every lockfile it writes and neither line can read the other's, so which one wrote it is a constraint, not a preference.

pnpm provides that package manager when the dependency pinned a version, or when the host cannot satisfy what the dependency needs — so a repository built with Yarn now installs on a machine that has only pnpm, while a host that already has a suitable one keeps using its own.

## What changes for a project coming from v11

| Command | v11 | v12 |
|---|---|---|
| `pnpm add yarn` | installs the npm package named `yarn` | records the project's package manager (the package is still reachable as `pnpm add yarn@npm:yarn@1.22.22`) |
| `pnpm add -g yarn` | installs Yarn Classic | installs the current Yarn line |
| `pnpm add -g node` / `pnpm add -g deno` | installs a wrapper package that downloads a build | installs that Node.js or Deno release |
| `pnx node` / `pnx deno` | runs the wrapper package | runs that release |
| a globally installed package manager | always the global copy | defers to a project's pin where there is one |
