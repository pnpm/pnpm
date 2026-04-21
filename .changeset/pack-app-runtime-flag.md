---
"@pnpm/releasing.commands": minor
"pnpm": minor
---

`pnpm pack-app`: replaced the `--node-version` flag with `--runtime`, which takes a `<name>@<version>` spec (e.g. `--runtime node@22.0.0`). The corresponding `pnpm.app.nodeVersion` key in package.json was renamed to `pnpm.app.runtime` with the same syntax. Only `node` is supported today; the prefix leaves room for future runtimes (`bun`, `deno`).

The previous `--node-version` flag silently inherited from pnpm's global `node-version` rc setting (which controls which Node runs user scripts), causing the wrong Node build to be embedded in SEAs for users who had that rc key set.
