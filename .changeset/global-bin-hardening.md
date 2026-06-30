---
"@pnpm/bins.remover": patch
"@pnpm/global.packages": patch
"@pnpm/global.commands": patch
"@pnpm/installing.commands": patch
"pnpm": patch
---

Hardened global package management:

- On Windows, removing or updating a global package now also cleans up the `node.exe` flavor of a bin, so a stale `node.exe` no longer survives on `PATH` after uninstall, and a new global install no longer silently overwrites an existing `node.exe`.
- `pnpm add -g pnpm@<version>` (and `@pnpm/exe@<version>`) is now rejected like the bare `pnpm` form, pointing to `pnpm self-update`.
- Dependency aliases read from a global package's manifest are validated before being joined onto `node_modules` paths, preventing a tampered manifest from escaping the install directory.
- Each global install group is created in its own freshly-made directory (no longer reusing a colliding or pre-existing path).
- Removing or updating a global package no longer unlinks a bin that belongs to a different globally installed package.
