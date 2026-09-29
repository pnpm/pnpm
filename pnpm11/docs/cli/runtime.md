---
id: runtime
title: "pnpm runtime <cmd>"
---

Manage runtimes.

Alias: `rt`

## Commands

### set

Install the specified version of a runtime.

```
pnpm runtime set <name> <version> [-g]
```

#### Supported runtimes

- `node` - Node.js
- `deno` - Deno
- `bun` - Bun

:::info

Since v11.0.0, installing a Node.js runtime (via `pnpm runtime set node …` or `node@runtime:<version>`) does not extract the bundled `npm`, `npx`, and `corepack` from the Node.js archive. This roughly halves the number of files pnpm has to hash, write to the CAS, and link during a runtime install. If you still need `npm`, install it separately with `pnpm add -g npm`.

:::

:::info

Since v11.22.0, resolving a Node.js version (through [`devEngines.runtime`](../package_json.md#devenginesruntime) or a `runtime:` specifier) is much faster. The per-version release metadata is cached in the [cache directory](./cache-path.md) after its signature is verified, and an exact stable version such as `runtime:22.23.2` no longer downloads the Node.js release index. A pinned runtime whose metadata was fetched once resolves without any network access, which removes the delay on the first `node` invocation in a project pinning an already-downloaded runtime.

:::

#### Examples

Install Node.js v22 globally:

```
pnpm runtime set node 22 -g
```

Install the LTS version of Node.js:

```
pnpm runtime set node lts -g
```

Install the latest version of Node.js:

```
pnpm runtime set node latest -g
```

Install a prerelease version of Node.js:

```
pnpm runtime set node nightly -g
pnpm runtime set node rc -g
pnpm runtime set node rc/22 -g
pnpm runtime set node 22.0.0-rc.4 -g
```

Install an LTS version of Node.js using its [codename]:

```
pnpm runtime set node argon -g
```

Install Deno:

```
pnpm runtime set deno 2 -g
```

Install Bun:

```
pnpm runtime set bun latest -g
```

[codename]: https://github.com/nodejs/Release/blob/main/CODENAMES.md

## Options

### --global, -g

Install the runtime globally.
