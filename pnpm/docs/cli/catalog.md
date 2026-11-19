---
id: catalog
title: pnpm catalog
---

Manages the [catalogs](../catalogs.md) of a workspace.

## Commands

### migrate

```sh
pnpm catalog migrate
```

Moves dependency specifiers into the default catalog of `pnpm-workspace.yaml` and replaces them in the workspace projects' `package.json` files with `catalog:`. It reads `dependencies`, `devDependencies`, `optionalDependencies`, and `peerDependencies` of every workspace project, the root project included.

A dependency moves only when every project that declares it directly uses the same specifier, and the default catalog has no different entry for it. Catalogs set by an `updateConfig` hook count. Any other dependency is listed and left unchanged. Dependencies that already use the `catalog:` or `workspace:` protocol, and local paths such as `file:` and `link:`, stay as they are.

The lockfile is not changed. Run `pnpm install` afterwards.
