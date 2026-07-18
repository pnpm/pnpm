## 11.15.0

### Minor Changes

- Optional peer dependencies declared only via `peerDependenciesMeta` (for example `debug`'s `supports-color` peer) are now resolved from a satisfying version already present in the dependency graph, the same way explicitly declared optional peer dependencies are. Previously such peers were only resolved this way when the package's metadata was read back from the lockfile, so an unrelated dependency change could rewrite peer resolutions across the whole lockfile.

### Patch Changes

- Updated `adm-zip` to prevent crafted ZIP archives from causing excessive memory allocation.

- `pnpm version -r` no longer writes a versioning-ledger entry with no consumed intents as a bare `intents:` key, which the next run failed to read with `ERR_PNPM_INVALID_VERSIONING_LEDGER`. Empty intent lists are now written as `intents: []`, and the ledger reader accepts the bare form left by earlier releases.

- Fixed pnpr workspace resolution to preserve project names and versions for `workspace:` dependencies.
