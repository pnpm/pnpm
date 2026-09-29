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

Do not run these commands through `sudo`. pnpm keeps global packages and configuration in the invoking user's home directory, so under `sudo` they operate on the root user's home instead of yours. Since v11.21.0, `pnpm setup`, `pnpm self-update`, and every command that modifies the global installation print a warning when run as root. Read-only commands such as `pnpm bin -g` are unaffected.

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
| `pnpm update -g [pkg]` | Update global packages (re-installs into new isolated directories) |
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
