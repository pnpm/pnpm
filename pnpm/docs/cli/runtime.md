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

Install the latest version of Node.js:

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

## Options

### --global, -g

Install the runtime globally.

### --save-dev, -D

Save the runtime to `devEngines.runtime`. This is the default.

### --save-prod, -P

Save the runtime to `engines.runtime`.
