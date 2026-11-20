---
id: docs
title: pnpm docs
---

Added in: v11.0.0

Aliases: `home`

Open a package's documentation (or homepage) in the browser.

```sh
pnpm docs <pkg>[@<version>]
```

pnpm fetches the package's metadata from the registry.

If the package does not declare a valid `homepage`, pnpm falls back to `https://npmx.dev/package/<name>`.

## Examples

Open the documentation for a published package:

```sh
pnpm docs lodash
```
