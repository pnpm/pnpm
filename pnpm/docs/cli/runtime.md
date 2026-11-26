---
id: runtime
title: "pnpm runtime <cmd>"
---

Manage runtimes.

Alias: `rt`

## Commands

### set

Install the specified version of a runtime.

```sh
pnpm runtime set <name> <version> [-g]
```

Without `-g`, the runtime is added to the current project as `<name>@runtime:<version>`. By default it goes to [`devEngines.runtime`](../package_json.md#devenginesruntime). With `--save-prod`, it goes to `engines.runtime`.

After installing, pnpm suggests running [`pnpm shim add <name>`](./shim.md) so that the bare command picks the project's runtime.

#### Supported runtimes

- `node` - Node.js
- `deno` - Deno
- `bun` - Bun

:::info

Installing a Node.js runtime (via `pnpm runtime set node …` or `node@runtime:<version>`) does not install the bundled `npm`, `npx`, and `corepack`. If you need `npm`, install it separately with `pnpm add -g npm`.

:::

:::info

The release metadata of a Node.js version is cached in the [cache directory](./cache-path.md) after its signature is verified. A pinned runtime whose metadata was fetched once resolves without network access.

:::

#### Examples

Install Node.js v22 globally:

```sh
pnpm runtime set node 22 -g
```

Install the LTS version of Node.js:

```sh
pnpm runtime set node lts -g
```

Install the newest stable release of Node.js:

```sh
pnpm runtime set node latest -g
```

Install a prerelease version of Node.js:

```sh
pnpm runtime set node nightly -g
pnpm runtime set node rc -g
pnpm runtime set node rc/22 -g
pnpm runtime set node 22.0.0-rc.4 -g
```

Install an LTS version of Node.js using its [codename]:

```sh
pnpm runtime set node argon -g
```

Install Deno:

```sh
pnpm runtime set deno 2 -g
```

Install Bun:

```sh
pnpm runtime set bun latest -g
```

[codename]: https://github.com/nodejs/Release/blob/main/CODENAMES.md

### unset

Remove a runtime installed by `runtime set`.

```sh
pnpm runtime unset <name> [<version>] [-g]
```

Without `-g`, pnpm removes the runtime dependency from the current project. A declaration must use the `runtime:` or `tool:` protocol; an ordinary dependency with the same name is left in place. Without `--save-dev` or `--save-prod`, pnpm removes every matching declaration from `devEngines.runtime` and `engines.runtime`. With a version, pnpm only removes a matching declaration. With `-g`, pnpm removes the installed runtime groups from the global packages directory and unlinks their bins. For Node.js, pnpm also removes the active link when it points at a removed version.

#### Examples

Unset Node.js v22 globally:

```sh
pnpm runtime unset node 22 -g
```

Unset all globally installed Bun versions:

```sh
pnpm runtime unset bun -g
```

Unset Deno from the current project:

```sh
pnpm runtime unset deno
```

## Options

### --global, -g

Apply the command to globally installed runtimes.

### --save-dev, -D

Target `devEngines.runtime`. This is the default for `set`.

### --save-prod, -P

Target `engines.runtime`.
