---
id: env
title: "pnpm env <cmd>"
---

:::warning Deprecated

`pnpm env` is deprecated. Use [`pnpm runtime`](./runtime.md) instead. For example, `pnpm env use --global lts` becomes `pnpm runtime set node lts -g`, and `pnpm env remove --global <version>` becomes `pnpm remove -g node`.

:::

Manages the Node.js environment.

## Commands

### use

Install and use the specified version of Node.js

Install the LTS version of Node.js:

```sh
pnpm env use --global lts
```

Install Node.js v16:

```sh
pnpm env use --global 16
```

Install a prerelease version of Node.js:

```sh
pnpm env use --global nightly
pnpm env use --global rc
pnpm env use --global 16.0.0-rc.0
pnpm env use --global rc/14
```

Install the latest version of Node.js:

```sh
pnpm env use --global latest
```

Install an LTS version of Node.js using its [codename]:

```sh
pnpm env use --global argon
```

[codename]: https://github.com/nodejs/Release/blob/main/CODENAMES.md

### remove, rm

Removes the specified versions of Node.js.

Usage example:

```sh
pnpm env remove --global 14.0.0
pnpm env remove --global 14.0.0 16.2.3
```

### list, ls

Print the Node.js versions available from the Node.js mirror that match a version range. Without a range, only the newest version is printed.

```sh
pnpm env list
```

Print the available Node.js v16 versions:

```sh
pnpm env list 16
```

## Options

### --global, -g

The changes are made systemwide. `use` and `remove` require this option. Without it, they fail with `ERR_PNPM_NOT_IMPLEMENTED_YET`.
