## 12.0.0-alpha.15

### Minor Changes

- Optional peer dependencies declared only via `peerDependenciesMeta` (for example `debug`'s `supports-color` peer) are now resolved from a satisfying version already present in the dependency graph, the same way explicitly declared optional peer dependencies are. Previously such peers were only resolved this way when the package's metadata was read back from the lockfile, so an unrelated dependency change could rewrite peer resolutions across the whole lockfile.

- Added `pnpm licenses` command to the Rust pacquet port to list package licenses in a tabular or JSON format.

### Patch Changes

- `pnpm version -r` no longer writes a versioning-ledger entry with no consumed intents as a bare `intents:` key, which the next run failed to read with `ERR_PNPM_INVALID_VERSIONING_LEDGER`. Empty intent lists are now written as `intents: []`, and the ledger reader accepts the bare form left by earlier releases.

- Fixed `pnpm install --frozen-lockfile` incorrectly failing with `ERR_PNPM_OUTDATED_LOCKFILE` when a workspace project declares `peerDependencies` that `auto-install-peers` resolves. With `auto-install-peers` enabled (the default), pnpm records those missing peers in the lockfile importer's `dependencies`; the frozen-lockfile freshness check now folds `peerDependencies` into the comparison instead of reporting the materialized peers as removed.

- Fixed Pacquet workspace commands to honor project filters, preserve complete lockfile state, and materialize only the selected dependency closure, including pnpr-backed installs.

- Fixed a lockfile corruption during non-frozen re-installs: when one workspace project reused a package's resolution from the lockfile and another project's edge to the same package was denied reuse (for example because it also depends on a direct dependency whose specifier changed), the denied edge could read the reused, dependency-less resolution from the shared wanted-dependency cache and record the package as a leaf. Its lockfile snapshot became empty (`{}`), its peer suffix was dropped, and none of its dependencies were linked, which later broke installs and builds consuming that lockfile [#13070](https://github.com/pnpm/pnpm/pull/13070).
