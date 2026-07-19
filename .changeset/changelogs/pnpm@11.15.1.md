## 11.15.1

### Patch Changes

- `pnpm install` now detects a `supportedArchitectures` change and re-evaluates previously skipped platform-specific optional dependencies, instead of reporting the project as up to date and leaving the packages for the old architecture set in place.

- `pnpm setup` now removes leftover v10-layout shims at the top of `PNPM_HOME`, so `pnpm self-update` no longer warns about a v10 installation layout after PATH has been migrated to the v11 `PNPM_HOME/bin` layout. Applies to both the TypeScript CLI and pacquet.

  In the TypeScript CLI, `self-update` also no longer treats a dangling legacy shim (one whose install target was garbage-collected) as a real v10 layout, so the warning can no longer fire on dead shim files.

  Closes pnpm/pnpm#12496.

- Completed pnpm runtime installation parity for Node.js, Deno, and Bun, including runtime failure policy, target architecture selection, and dependency runtime engines. Runtime failure overrides now preserve explicit runtime dependencies without matching engine entries.

- Fixed `pnpm install` running out of memory while resolving large dependency graphs [#8441](https://github.com/pnpm/pnpm/issues/8441). The resolver kept full registry documents — per-version readmes, scripts, descriptions, and other install-irrelevant bulk — in memory for every package fetched with full metadata (optional dependencies, and packages re-fetched for `minimumReleaseAge`'s publish timestamps). Every retained document is now condensed down to the field set installation actually reads, which reduces peak resolution memory by several times on workspaces with more than a thousand packages.

- When a dependency's build script fails under `enableGlobalVirtualStore`, the global virtual store directory it was being built in is now removed for scoped packages too. Previously the cleanup resolved one directory level short of the hash directory for a scoped name, leaving a half-built directory behind that later installs would reuse.

- Fixed `pnpm login`, `pnpm adduser`, and `pnpm logout` against a registry hosted under a URL subpath (e.g. `https://example.com/npm/registry`) when the configured URL has no trailing slash. Such URLs were left unnormalized, so the last path segment was dropped when building the login and token endpoints and the auth token was stored under a truncated key. Registry URLs with a path now always get a trailing slash appended during normalization, matching how root-level registry URLs are handled.
