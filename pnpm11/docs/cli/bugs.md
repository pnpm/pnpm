---
id: bugs
title: pnpm bugs
---

Added in: v11.1.0

Opens a package's bug tracker URL in the browser.

```sh
pnpm bugs [<pkg> ...]
```

## Aliases

`pnpm issues` (added in v11.10.0) is an alias of `pnpm bugs`.

When run without arguments inside a package directory, it opens the bug tracker for the current project (using the `bugs` field from `package.json`).

When one or more package names are passed, pnpm fetches each package's metadata from the registry and opens its bug tracker.

If a package does not declare a `bugs` field, pnpm falls back to `<repository>/issues` derived from the `repository` field.

## Examples

Open the bug tracker for a published package:

```sh
pnpm bugs lodash
```

Open the bug tracker for multiple packages at once:

```sh
pnpm bugs react react-dom
```

Open the bug tracker for the current project:

```sh
pnpm bugs
```
