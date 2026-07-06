---
"@pnpm/engine.runtime.commands": patch
"pnpm": patch
---

`pnpm runtime set <name> <version>` now validates its arguments: the name must be `node`, `deno`, or `bun`, and the version must not contain a comma. Previously these were interpolated straight into a `pnpm add` selector, where an unsupported name or a comma (e.g. `node 22,is-positive`) could be misread as a list of packages or a local directory and install unintended packages or bins.
