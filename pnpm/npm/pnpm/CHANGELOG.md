# pacquet

## 12.0.0-alpha.9

### Minor Changes

- Added `versioning.epics` to `pnpm-workspace.yaml`. An epic ties a group of member packages to a lead package, constraining every member's major version to a band derived from the lead's major: while the lead is on major `M`, members live in `M*100 … M*100+99`. Members move independently inside the band (patch, minor, and a `major` intent that stays in-band); a bump that would carry a member past the band ceiling is rejected until the lead advances its own major. When a release plan takes the lead to a new stable major, every member re-bases to the band floor in the same plan. Membership is matched with pnpm's package selectors — name globs, `./`-prefixed directory globs, and `!`-prefixed negations.

- Added the `deprecate` and `undeprecate` commands for setting or removing the `deprecated` message on a package version (or semver range) in the registry, with support for `--registry` and `--otp`.

- Added the `star`, `unstar`, and `stars` commands. `star` and `unstar` mark or unmark a package as a favorite (falling back to editing the packument's `users` map on registries without the star endpoints), and `stars` lists the packages starred by the current or a specified user.

- Added support for the `tokenHelper` auth setting, matching the TypeScript CLI. A `tokenHelper` configured in `~/.npmrc` or the global pnpm `auth.ini` names a command pacquet runs to obtain a registry token; the command runs lazily (only when a request to that registry is actually made), is given a 60-second time limit, and its output becomes the `Authorization` header. A `tokenHelper` in a workspace or project `.npmrc`, or supplied through a URL-scoped environment variable, is refused so a checked-in config can't run an arbitrary command.

### Patch Changes

- Use macOS native DNS resolution with bounded concurrency so installs respect scoped and VPN-provided resolvers.

- Fixed an injected workspace dependency (`injectWorkspacePackages: true`) incorrectly staying as `file:` instead of deduping back to `link:` when an unrelated, ordinary shared dependency resolved to a peer-suffixed variant for the target project's own copy but not for the injected occurrence. See pnpm/pnpm#10433.

- `pnpm deploy` now supports workspaces that use catalogs.

- Fixed `pnpm deploy` with a shared lockfile so local `file:` tarball dependencies keep their package name in the generated deploy lockfile. This prevents warm-store deploys from failing with `ERR_PNPM_UNEXPECTED_PKG_CONTENT_IN_STORE` when the tarball filename includes the version.

- Fixed installs failing on Windows when the global virtual store is enabled. The `<store>/links/<scope>/<name>/<version>/<hash>` slot path is formatted with `/` separators (it doubles as a cross-platform canonical id), and those forward slashes were reaching `CreateSymbolicLinkW`, which rejects forward-slash paths with `ERROR_DIRECTORY` (os error 267). The slot path is now expanded into native path components before any filesystem call.

- `pnpm pack` and `pnpm publish` now apply the `beforePacking` pnpmfile hook to the manifest before a package is packed, matching the TypeScript CLI.

- Resolve `catalog:` specifiers in the dependencies of injected workspace packages (`injectWorkspacePackages: true`). Previously such a child spec bypassed catalog resolution and failed with `ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER`, matching the TypeScript CLI.

- `pnpm list --json` and `pnpm list --parseable` now report extraneous packages — packages present in `node_modules` but absent from the lockfile — under `unsavedDependencies`, matching the TypeScript CLI.

- Fixed `workspace:` dependencies failing to resolve when they point at a named workspace package whose `package.json` has no `version` (or a `null` version). Such packages are now indexed as version `0.0.0`, matching the TypeScript CLI, so specs like `workspace:*` and `workspace:0.0.0` resolve instead of failing with a misleading "no package named" error.

- `pnpm publish` again sends the package's README to the registry as metadata, so registries can render it on the package page. The readme is always included in the published metadata (matching the npm CLI), while the `embed-readme` setting continues to control only whether the readme is written into the `package.json` inside the tarball. This restores the behavior that was lost when publishing became fully native. Closes pnpm/pnpm#12966.

- Fixed the incremental install fast path wrongly reporting "already up to date" — skipping re-resolution — when a `package.json`, `.pnpmfile.cjs`, or patch file was edited immediately after an install. The freshness check compared file modification times against a wall-clock timestamp, which broke in two ways: on a machine whose wall clock and filesystem clock disagree (seen on some CI runners) the timestamp could sit ahead of a later edit's mtime, and a fast install could write its lockfile in the same millisecond as the subsequent edit. The check now records the baseline from filesystem mtimes and compares at nanosecond precision.

- Fixed the dependency status check wrongly reporting "up to date" when a `package.json`, `.pnpmfile.cjs`, or patch file was edited in the same second as the previous install, on filesystems that record mtimes at whole-second resolution (for example ext4 with 128-byte inodes). The optimistic repeat-install fast path and `verify-deps-before-run` compared mtimes strictly, so a same-second edit whose mtime rounded down looked unchanged and re-resolution was skipped. Such a file's whole second is now treated as possibly-modified, falling through to the content check; behavior on sub-second filesystems is unchanged.

- Retry package metadata requests when a registry or proxy returns `304 Not Modified` to an unconditional request, preventing false `ERR_PNPM_CACHE_MISSING_AFTER_304` failures [pnpm/pnpm#12882](https://github.com/pnpm/pnpm/issues/12882).

  If the retry also returns `304`, report `ERR_PNPM_META_NOT_MODIFIED_WITHOUT_CACHE` instead.

- The `pnpm` wrapper's install script exits without error in the pnpm monorepo checkout, where the per-platform binary packages are not generated.

- Limit modern deploy lockfiles and localized virtual stores to dependencies reachable from the selected dependency groups.

- `pnpm pack` now respects workspace-root `.npmignore` and `.gitignore` files when packing workspace packages.
