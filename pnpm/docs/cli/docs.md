---
id: docs
title: pnpm docs
---

Aliases: `home`

Open a package's documentation (or homepage) in the browser.

```sh
pnpm docs <pkg>[@<version>]
```

pnpm fetches the package's metadata from the registry.

A `docs` or `home` script in `package.json` overrides the built-in command and receives positional arguments such as `check` in `pnpm docs check`. [`pnpm pm docs`](./pm.md) and `pnpm pm home` run the built-in command regardless.

If the package does not declare a valid `homepage`, pnpm falls back to `https://npmx.dev/package/<name>`. When no browser can be opened, pnpm prints the URL.

## Options

### --registry &lt;url\>

The registry to fetch the package's metadata from.

## Examples

Open the documentation for a published package:

```sh
pnpm docs lodash
```
