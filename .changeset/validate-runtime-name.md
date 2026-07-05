---
"@pnpm/engine.runtime.commands": patch
"pnpm": patch
---

`pnpm runtime set <name> <version>` now rejects a runtime name that is not `node`, `deno`, or `bun`. Previously an unsupported name (including a comma-separated list or a path-like value such as `./foo`) was forwarded to `pnpm add`, where it could be misread as a list of packages or a local directory and install unintended packages or bins.
