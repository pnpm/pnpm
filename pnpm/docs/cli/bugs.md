---
id: bugs
title: pnpm bugs
---

Opens a package's bug tracker URL in the browser.

Aliases: `issues`

```sh
pnpm bugs [<pkg> ...]
```

When run without arguments inside a package directory, it opens the bug tracker for the current project (using the `bugs` field from `package.json`).

When one or more package names are passed, pnpm fetches each package's metadata from the registry and opens its bug tracker.

If a package does not declare a `bugs` field, pnpm falls back to `<repository>/issues` derived from the `repository` field.

## Options

### --registry &lt;url\>

The registry to fetch the named packages' metadata from.

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
