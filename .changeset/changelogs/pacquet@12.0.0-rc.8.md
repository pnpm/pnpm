## 12.0.0-rc.8

### Minor Changes

- `packageImportMethod: auto` now tries hardlinks before cloning on Linux. A reflink materializes a new inode and copies extent bookkeeping inside the filesystem's metadata trees, where a hardlink is one directory entry — on btrfs this roughly halves the time an install spends materializing `node_modules` from a warm store. ext4 installs are unchanged (cloning was never supported there, so `auto` already hardlinked), and macOS keeps clone-first, where APFS `clonefile` is the platform's cheap primitive. Cloning remains the fallback when the store refuses hardlinks, and remains available explicitly via `packageImportMethod: clone`.

  This ships with pnpm 12 only: pnpm 11's importer deliberately keeps clone-first, since changing what the default materializes on disk is not a point-release change.

### Patch Changes

- `pnpm approve-builds` now removes `onlyBuiltDependencies`, `onlyBuiltDependenciesFile`, `neverBuiltDependencies`, and `ignoredBuiltDependencies` from `pnpm-workspace.yaml` when it writes `allowBuilds`. Those settings were replaced by `allowBuilds` in pnpm 11 and silently ignored since, so a workspace migrated from pnpm 10 kept them around looking active.

- `pnpm audit` no longer reports a patched version that was never published or is deprecated. The inferred patched range (e.g. `>=4.17.24` from `<=4.17.23`) is now checked against the registry packument, and the report is corrected to the lowest non-deprecated published version that satisfies it (e.g. `>=4.18.1` when `4.17.24` does not exist and `4.18.0` is deprecated). When no published version satisfies the range, the report shows `Patched versions: None`. This also prevents `pnpm audit --fix` from adding overrides or `minimumReleaseAgeExclude` entries for patches that do not exist [#13824](https://github.com/pnpm/pnpm/issues/13824).

  `pnpm audit --fix` and `pnpm audit --fix update` no longer add a `minimumReleaseAgeExclude` entry when the registry packument shows that the minimum patched version was never published. Previously such entries were written for versions that do not exist, which would have let a later publish of that version bypass the `minimumReleaseAge` gate [#11563](https://github.com/pnpm/pnpm/issues/11563).

  The `--json` output of `pnpm audit` now returns `patched_versions: null` for advisories whose inferred patch is not available (never published, skipped, yanked, or deprecated), making it easier for tooling to distinguish "no fix available" from "fix available at version X".

- Re-fetch full registry metadata when `minimumReleaseAge` is enabled and an abbreviated packument's `time` map omits timestamps for some versions. This prevents mature versions from being filtered out and resolution from falling back to the lowest matching version [pnpm/pnpm#13741](https://github.com/pnpm/pnpm/issues/13741).

- A config dependency carrying an inline integrity (the `<version>+<integrity>` form, or the object form without a `tarball`) now takes its tarball URL from the registry's packument instead of deriving it from the registry URL, so migrating one costs an extra metadata request. On a registry that serves tarballs from a path pnpm cannot derive, GitLab's group endpoint for one, installing such a config dependency failed with a 404 while the same package installed fine as a regular dependency [#13765](https://github.com/pnpm/pnpm/issues/13765).

- Don't treat files like `license16.json` as a package license when deciding if the workspace LICENSE file should be included in the packed package.

- Reduced warm update overhead by limiting virtual-store bin linking and ignored-script build bookkeeping to packages materialized by the current install.

- `pnpm init` now pins the exact pnpm version instead of a `^` range, and records it in the `packageManager` field alongside `devEngines.packageManager`. Corepack reads only `packageManager` and accepts nothing but an exact version, so it rejected the generated `package.json` with "expected a semver version" [pnpm/pnpm#13969](https://github.com/pnpm/pnpm/issues/13969). A package created inside an existing workspace is still left unpinned — it follows the pin at the workspace root — and `--no-init-package-manager` still scaffolds a manifest without any pin. In pnpm 12, `pnpm init` also honors `initType` and its `--init-type` flag, so the manifest it writes is the same one pnpm 11 writes.

- `node-linker=hoisted` installs no longer produce broken layouts on graphs with version conflicts. Three hoister fixes, aligning with `@yarnpkg/nm` (which the TypeScript CLI delegates to):

  - A version-conflicted package depended on by several packages kept its conflicting transitive dependencies under only one of the dependents, so requiring them through any other dependent resolved the wrong (root-hoisted) version — for example an ESM `parse-entities@4` resolving `character-entities-legacy` v1 instead of v3, which crashes with `ERR_IMPORT_ATTRIBUTE_MISSING` on Node.js 22. Hoist decisions are now made per parent path on decoupled copies (ports upstream's `decoupleGraphNode`).
  - Peer-resolution variants of one package version now collapse onto a single copy (ports pnpm v11's `depPathByPkgId` mapping) instead of conflict-nesting a copy under every dependent — on peer-variant-heavy graphs (such as `bit`'s) the old behavior also made the per-path walk explode.
  - Hoisting no longer shadows names a subtree resolves through an ancestor directory: a candidate is refused when a nearer ancestor holds a different version of its name (upstream's "filled by parent" scan) or when the hoist root's subtree already resolves that name from above (upstream's `usedDependencies` gate).

- `pnpm update --no-save <pkg>@<version>` now keeps the manifest's declared importer specifier in `pnpm-lock.yaml` when the requested version satisfies that range, so a subsequent `--frozen-lockfile` install no longer fails because the lockfile records the requested version as the specifier.

- Reduced registry metadata requests during dependency resolution by reusing cached metadata when lockfile preferences prove that no uncached version can win [pnpm/pnpm#13976](https://github.com/pnpm/pnpm/issues/13976).

- Improved install performance: the store-index writer's shutdown now overlaps the install's final lockfile and `.modules.yaml` writes instead of extending the install's tail.

- A setting in the global `config.yaml` that pnpm does not read from that file, or that is written in kebab-case instead of camelCase, is now reported instead of being ignored silently.

- A forced full re-resolution (config changes the fast lockfile update cannot absorb, such as a changed override or `packageExtensions`) no longer moves dependencies whose recorded versions still satisfy their ranges. The prior lockfile now pins each still-satisfied edge even when its recorded subtree cannot be reused wholesale, so open ranges like `@types/node: "*"` keep their locked versions instead of collapsing onto the highest locked version and churning the lockfile.

- Improved fresh resolution performance when package metadata is already cached.

- Improved fresh installs by reusing the store index and verified-files cache during dependency materialization.

- A runtime installed through `devEngines.runtime` now matches the host when `supportedArchitectures` lists several platforms. Listing `os: [darwin, linux]` and `cpu: [x64, arm64]` used to install the runtime built for the first entry of each list, so a machine running Linux on arm64 got a macOS x64 Node.js that could not execute [#13898](https://github.com/pnpm/pnpm/issues/13898).

- `pnpm self-update <tag>` no longer downgrades when the dist-tag points at the pnpm version already running and that version is younger than `minimumReleaseAge`. The maturity cutoff moved the tag back to the previous mature release, so `pnpm self-update next-12` on v12.0.0-rc.4 switched to v12.0.0-rc.3.

- Improved install performance: large tarballs are now verified and extracted while they download, so the biggest packages — whose downloads finish last — no longer add their whole extraction to the end of the install.
