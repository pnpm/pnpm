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

If the package does not declare a valid `homepage`, pnpm falls back to `https://npmx.dev/package/<name>`. When no browser can be opened, pnpm prints the URL.

## Options

### --registry &lt;url\>

The registry to fetch the package's metadata from.

## Examples

Open the documentation for a published package:

```sh
pnpm docs lodash
```
