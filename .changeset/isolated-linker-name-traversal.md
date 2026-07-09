---
"@pnpm/resolve-dependencies": patch
"@pnpm/deps.graph-builder": patch
"pnpm": patch
---

Fixed a path traversal vulnerability where a dependency whose manifest `name` was a scoped path traversal (e.g. `@x/../../../<path>`) could be written outside `node_modules` to an attacker-controlled location during `pnpm install`, even with `--ignore-scripts`. The isolated linker now validates the package name before using it as a directory name, matching the existing protection in the hoisted linker.
