---
id: docs
title: pnpm docs
---

Added in: v11.0.0

Aliases: `home`

Open a package's documentation (or homepage) in the browser.

```sh
pnpm docs [<pkg> ...]
```

When run without arguments inside a package directory, it opens the documentation for the current project.

If the package does not declare a valid `homepage`, pnpm falls back to `https://npmx.dev/package/<name>`.

## Examples

Open the documentation for a published package:

```sh
pnpm docs lodash
```

Open the documentation for multiple packages at once:

```sh
pnpm docs react react-dom
```

Open the documentation for the current project:

```sh
pnpm docs
```
