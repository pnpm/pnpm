## 12.0.0-rc.0

### Minor Changes

- Running `pnpm setup`, `pnpm self-update`, or a command that modifies the global installation (such as `pnpm add --global`) through `sudo` now fails with `ERR_PNPM_SUDO_NOT_SUPPORTED` instead of silently operating on the root user's home directory. pnpm keeps global packages and configuration in the invoking user's home directory, so these commands never need root permissions. Read-only global commands (such as `pnpm bin --global`) still work under sudo.

### Patch Changes

- Archive entries whose paths use `\` as a separator are now read the same way pnpm reads them. A nested path spelled `bin\tool.js` by Windows publishing tooling resolves to `bin/tool.js`, and a path traversal spelled with backslashes is rejected instead of being stored verbatim.

- Fixed `file:` dependencies not being re-copied when their source directory changed. A `file:` dependency is copied into the store at install time rather than symlinked, so editing the local package's files and running `pnpm install` again left the previous copy in place — the lockfile is unchanged by such an edit, so the install treated the tree as up to date.

- Write blocked-build approval scaffolding to the discovered workspace manifest when using per-project lockfiles.

- Concurrent installs sharing a global virtual store no longer fail with `failed to remove existing directory ... prior to swap: Directory not empty`, and no longer briefly remove a package directory another process is reading.

- Fixed `link:` dependencies under `enableGlobalVirtualStore` so linked children are materialized and slots remain isolated by their resolved link targets.

- A headless install (`--frozen-lockfile`) now creates the command shims for a publicly hoisted workspace package's `bin`, matching what a normal install already did and what pnpm's own headless install does. Previously those shims were missing until the next non-frozen install.

- `pnpm fetch`, and any install run with `virtualStoreOnly`, no longer writes a `.pnp.cjs` loader under `nodeLinker: pnp`. These installs populate the virtual store without linking the project, so the loader would have claimed the project resolves out of a store it was never linked into. The importer links and `node_modules/.package-map.json` were already skipped; the PnP loader now follows the same rule.

- Prevent pnpm from removing project files when `modulesDir` resolves to the project root.

- Fixed `pnpm install` ignoring a `pnpm-lock.yaml` that carries a leading env lockfile document when the file has CRLF line endings or a UTF-8 byte order mark, as a `core.autocrlf` checkout on Windows produces. The lockfile was reported as broken with `multiple YAML documents detected` and every dependency was re-resolved from the registry [#13606](https://github.com/pnpm/pnpm/issues/13606).

- When no directory above the project accepts a hard link — inside an AI agent sandbox that only grants write access to the project, or a container with just the project mounted writable — the default store is now created at `<project>/node_modules/.pnpm-store` instead of in the pnpm home directory. In those environments the home store is either read-only or on another volume, which forces every package to be copied instead of hard linked [#13525](https://github.com/pnpm/pnpm/issues/13525).

- A stray non-directory entry in `node_modules` no longer fails an install. Files placed next to the installed dependencies are skipped rather than reported as an unreadable manifest.
