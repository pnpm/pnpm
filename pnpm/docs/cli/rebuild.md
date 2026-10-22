---
id: rebuild
title: pnpm rebuild
---

Aliases: `rb`

Re-runs the lifecycle (build) scripts of installed dependencies. The rebuild keeps the dependency groups that the last install included.

```sh
pnpm rebuild [<pkg>...]
```

With package names, only those packages are rebuilt. Without them, every dependency that requires a build is rebuilt.

A `rebuild` script in `package.json` overrides the command, and an `rb` script overrides `pnpm rb`. See [`pnpm pm`](./pm.md).

## Options

### --pending

Rebuild only the packages that were not built during installation, such as under `--ignore-scripts`.

```sh
pnpm rebuild --pending
```

### --recursive, -r

This command runs the **pnpm rebuild** command in every package of the monorepo.

### --no-bail

See [`--no-bail`](./recursive.md#--no-bail).

### --filter &lt;package_selector\>

[Read more about filtering.](../filtering.md)
