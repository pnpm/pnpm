---
id: global-packages
title: Global Packages
---

Global packages are CLI tools and utilities installed system-wide with `pnpm add -g`. In pnpm v11, global package management was redesigned for better isolation and reliability.

## Installing global packages

```sh
pnpm add -g <pkg>
```

For example:

```sh
pnpm add -g typescript prettier eslint
```

:::caution

Do not run these commands through `sudo`. pnpm keeps global packages and configuration in the invoking user's home directory. Commands that modify the global installation fail under `sudo` with `ERR_PNPM_SUDO_NOT_SUPPORTED`; read-only commands such as `pnpm bin -g` are unaffected.

:::

## Isolated installations

Each globally installed package (or group of packages installed together) gets its own isolated installation directory with its own `package.json`, `node_modules/`, and lockfile. This prevents global packages from interfering with each other through peer dependency conflicts, hoisting changes, or version resolution shifts.

Isolated installations are stored at `{pnpmHomeDir}/global/v11/{hash}/`, where the hash is derived from the set of packages installed together.

For example, running the following two commands:

```sh
pnpm add -g typescript
pnpm add -g prettier
```

creates two separate isolated installations — `typescript` and `prettier` each get their own `node_modules` tree and cannot affect each other's dependency resolution.

Installing multiple **space-separated** packages in a single command also creates a separate isolated install for each one:

```sh
pnpm add -g eslint prettier
```

`eslint` and `prettier` each get their own `node_modules` tree and lockfile and can be removed independently — `pnpm remove -g eslint` leaves `prettier` untouched.

To bundle multiple packages into the *same* isolated install — so they share a `node_modules` tree and lockfile, resolve peer dependencies against each other, and are removed together — pass them as a **comma-separated** list:

```sh
pnpm add -g eslint,prettier
```

Here `eslint` and `prettier` form a single install group. Removing either with `pnpm remove -g` removes the whole group.

The two forms can be mixed. For example:

```sh
pnpm add -g eslint,prettier typescript
```

bundles `eslint` and `prettier` into one isolated install while installing `typescript` on its own.

## Directory layout

The contents of `{pnpmHomeDir}/global/v11/` look like:

```text
{pnpmHomeDir}/global/v11/
├── {hash-A}              → symlink → ./{hash-A-target}/
├── {hash-A-target}/      ← isolated install dir
│   ├── package.json      ← lists the packages installed together
│   ├── pnpm-lock.yaml    ← lockfile for this install group
│   └── node_modules/
│       ├── <pkg>/        ← top-level dep, symlinked into the global virtual store
│       └── .pnpm/
├── {hash-B}              → symlink → ./{hash-B-target}/
├── {hash-B-target}/      ← another isolated install dir
└── store/                ← shared global virtual store
    └── ...
```

- The `{hash}` entries are symlinks; pnpm scans for them to enumerate active installs.
- The targets are real directories that act as ordinary pnpm projects — each has its own `package.json` and lockfile.
- The shared `store/` directory holds the [global virtual store](./global-virtual-store.md). Each install group's direct dependencies — the entries at the root of its `node_modules/` — are symlinks into that store, so the actual package contents are shared rather than copied per group.
- Bin shims live in `{pnpmHomeDir}/bin/` and point through the appropriate install group's `node_modules`.

When a package is removed or its install group is replaced, the hash symlink is updated and orphaned target directories are eventually cleaned up by `pnpm store prune`.

## Listing global packages

```sh
pnpm list -g
pnpm list -g --json        # machine-readable
pnpm list -g --parseable   # paths only
```

Because each install group has its own lockfile, listing across multiple groups can only reliably aggregate the top-level packages they were installed with — transitive dependency trees from different groups can't be coherently merged. As a result:

- `pnpm list -g` (default `--depth=0`) always works and shows every globally installed package.
- `pnpm list -g --depth=<n>` (with `n > 0`) shows the full dependency tree only when:
  - there is just one global install group, or
  - a positional argument narrows the request to a single install group, e.g. `pnpm list -g eslint --depth=1`.

If `--depth>0` is requested but the request can't be narrowed to a single install group, pnpm errors with `ERR_PNPM_GLOBAL_LS_DEPTH_NOT_SUPPORTED`.

## Managing global packages

| Command | Description |
|---|---|
| `pnpm add -g <pkg>` | Install a package globally |
| `pnpm remove -g <pkg>` | Remove a globally installed package (if it was bundled into an install group, the whole group is removed) |
| `pnpm update -g [pkg]` | Update global packages (re-installs a package into a new isolated directory when its resolved dependency graph changed) |
| `pnpm list -g` | List all globally installed packages |

:::note

`pnpm install -g` (without arguments) is not supported. Use `pnpm add -g <pkg>` to install specific packages.

:::

## Binaries location

Globally installed binaries are stored in a `bin` subdirectory of `PNPM_HOME` (i.e., `$PNPM_HOME/bin/`). This keeps the `PNPM_HOME` directory clean — internal directories like `global/` and `store/` don't pollute shell autocompletion when `PNPM_HOME` is on PATH.

After upgrading to pnpm v11, run [`pnpm setup`](./cli/setup.md) to update your shell configuration so that `$PNPM_HOME/bin` is on your PATH.

You can check the current global bin directory with:

```sh
pnpm bin -g
```

## Project-aware global bins

Added in: v12.0.0-rc.2 (pnpm v12 only)

Global commands can follow the project you are standing in. When you run a global command from inside a project that asks for a different version of the same tool, pnpm runs the version the project asks for.

The most useful case is Node.js itself. Given a project that pins its runtime:

```json title="package.json"
{
  "devEngines": {
    "runtime": {
      "name": "node",
      "version": "^22.0.0",
      "onFail": "download"
    }
  }
}
```

a bare `node` inside that directory runs Node.js 22, even if your global Node.js is a different major:

```sh
cd ~/projects/legacy-app
node --version   # v22.x.x — the version the project pins
cd ~
node --version   # your globally installed version
```

No shell hooks, `.bashrc` edits, or `use`-style commands are involved — the shims pnpm writes into the global bin directory do the dispatch themselves.

### How a version is chosen

pnpm walks up from the current working directory to the nearest project that provides the command, then:

* For the **runtimes** (`node`, `deno`, `bun`), only a runtime pin counts — [`devEngines.runtime`](./package_json.md#devenginesruntime), then `engines.runtime`. For `node`, since v12.7.0, a `.node-version` or `.nvmrc` file also counts (see [below](#nvmrc-and-node-version)). The pinned version is downloaded into the [global virtual store](./global-virtual-store.md) on demand and executed directly. The project's `node_modules/.bin` is never consulted for a runtime, so a dependency cannot supply the `node` you run.
* For the **[package managers](./package-managers.md)** (`npm`, `yarn`, `bun`), added in v12.0.0-rc.6, the project's `packageManager` or [`devEngines.packageManager`](./package_json.md#devenginespackagemanager) pin counts, and pnpm provisions that version on demand. The pin outranks a copy of that package manager installed globally, because it is the project's own statement of what installs it.
* For **any other package**, the project's `node_modules/.bin/<name>` is used.

Directories inside the pnpm home are skipped, since global installs are not projects.

If the project provides nothing, or the lookup is declined, the globally installed version runs — commands never fail merely because dispatch did not apply.

### `.nvmrc` and `.node-version`

Added in: v12.7.0

The `node` shim also reads the Node.js version from a `.node-version` or `.nvmrc`
file, so projects set up for nvm, fnm, or n work without a `devEngines.runtime`
entry:

```text title=".nvmrc"
22
```

The nearest directory that declares a Node.js runtime in any of these ways
decides the version. Within one directory, `package.json` takes precedence
over `.node-version`, which takes precedence over `.nvmrc`.

Both files accept a version or range, optionally prefixed with `v`, as well as
`node`, `stable`, `lts/*`, and LTS codenames such as `lts/jod`. Comments are
ignored. A value that only nvm can act on, such as `system`, `default`, or a
custom alias, is ignored, and the directory counts as unpinned.

### Trust

A project you `cd` into is not automatically allowed to run its own binaries in place of your global ones.

This section describes the default `auto` policy. Under `auto`:

* A **stable Node.js release** is verified against the Node.js release team's signatures before it runs, so it switches without asking. (On musl-based systems the matching builds are unsigned, so they fall under the prompt instead.)
* **Everything else** — Deno, Bun, Node.js prereleases, and any ordinary package you enable — asks once, per project and per candidate:

  ```text
  The project at "/home/user/projects/app" provides its own "tsc", which will be used
  instead of the globally installed one.
  Do you trust this project? [y/N]
  ```

Answers are remembered in a machine-local registry, keyed to the project directory *and* to a fingerprint of the exact binary. If the binary or the providing package changes, pnpm asks again rather than reusing the old approval.

In CI and any non-interactive session, the question cannot be asked, so the global version runs and nothing is recorded.

The other two [policies](./settings/other.md#globalshims) change which candidates reach that question. `prompt` sends every candidate through it, including a signature-verified stable Node.js release; answers are still remembered, so it asks once per project and candidate rather than on every run. `always` skips the question entirely and switches straight away — which is also what makes it usable in CI, where a prompt would otherwise fall back to the global version.

There is one more guard that applies regardless of policy: the project's command must come from the **same package** as the global one. A project that ships a lookalike `tsc` from some other package does not match, and your global `tsc` runs.

### Which packages participate

Only the runtimes participate by default. Since v12.0.0-rc.6, installing a package manager globally (`pnpm add -g yarn`) also adds an entry for it, so it follows a project's pin the way a globally installed Node.js already follows `devEngines.runtime`. An entry you set yourself, including `false`, is left as you set it.

Enable others with [`globalShims`](./settings/other.md#globalshims), keyed by the providing package's name:

```yaml title="~/.config/pnpm/config.yaml"
globalShims:
  typescript: true
```

Because pnpm decides at install time which bins to give dispatching shims, a package that is already installed globally needs to be reinstalled after you enable it:

```sh
pnpm add -g typescript
```

Turning a package *off* needs no reinstall — the setting is re-read on every dispatch. The same setting disables dispatch per package, or entirely with `globalShims: false`. For a single command, set `PNPM_SHIM_BYPASS=1`:

```sh
PNPM_SHIM_BYPASS=1 node --version
```

A package that is not installed globally at all can still get a dispatching command, with [`pnpm shim add`](./cli/shim.md) — that is what makes `yarn` work inside a Yarn project on a machine that has only pnpm.

:::note

`globalShims` is deliberately not read from a project's `pnpm-workspace.yaml` — only from the global configuration file, the pnpm home directory's own `pnpm-workspace.yaml`, and the environment. See the [setting's documentation](./settings/other.md#globalshims) for details.

:::

## Global virtual store

Global installs use the [global virtual store](./global-virtual-store.md). Packages are stored at `{storeDir}/links` and shared across global installations. This avoids redundant fetches when multiple global packages depend on the same libraries.

## Registering local packages globally

To make a local package's binaries available system-wide, use `pnpm add -g .` from the package directory:

```sh
cd ~/projects/my-tool
pnpm add -g .
```

This registers the package's `bin` entries so they can be invoked from anywhere. See [`pnpm link`](./cli/link.md#add-a-binary-globally) for more details.

## Build script approval

Global packages that have build scripts (e.g., `postinstall`) require approval. When you install a global package that needs to run build scripts, pnpm will prompt you to approve or deny the build interactively.

You can also pre-approve builds using the `--allow-build` flag:

```sh
pnpm add -g --allow-build=esbuild esbuild
```

Since v11.24.0, [`pnpm approve-builds --global`](./cli/approve-builds.md#--global--g)
lets you decide after the fact instead. It collects the packages awaiting approval from every
install group, asks about them once, and writes one `allowBuilds` policy into the
`pnpm-workspace.yaml` of the global packages directory — then rebuilds only the
groups that contain something you approved:

```sh
pnpm approve-builds -g
```
