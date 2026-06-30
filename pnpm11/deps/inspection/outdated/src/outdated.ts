import {
  type CatalogResolutionFound,
  matchCatalogResolveResult,
  resolveFromCatalog,
  type WantedDependency,
} from '@pnpm/catalogs.resolver'
import type { Catalogs } from '@pnpm/catalogs.types'
import { createMatcher } from '@pnpm/config.matcher'
import { parseOverrides } from '@pnpm/config.parse-overrides'
import { LOCKFILE_VERSION } from '@pnpm/constants'
import * as dp from '@pnpm/deps.path'
import { PnpmError } from '@pnpm/error'
import { createReadPackageHook } from '@pnpm/hooks.read-package-hook'
import type { ResolveLatestDispatcher } from '@pnpm/installing.client'
import {
  getLockfileImporterId,
  type LockfileObject,
  type ProjectSnapshot,
} from '@pnpm/lockfile.fs'
import { getAllDependenciesFromManifest } from '@pnpm/pkg-manifest.utils'
import {
  DEPENDENCIES_FIELDS,
  type DependenciesField,
  type DepPath,
  type IncludedDependencies,
  type PackageManifest,
  type PackageVersionPolicy,
  type ProjectManifest,
} from '@pnpm/types'
import semver from 'semver'

export * from './createManifestGetter.js'

export interface OutdatedPackage {
  alias: string
  belongsTo: DependenciesField
  current?: string // not defined means the package is not installed
  latestManifest?: PackageManifest
  packageName: string
  wanted: string
  workspace?: string
}

export async function outdated (
  opts: {
    catalogs?: Catalogs
    compatible?: boolean
    currentLockfile: LockfileObject | null
    resolveLatest: ResolveLatestDispatcher
    ignoreDependencies?: string[]
    include?: IncludedDependencies
    lockfileDir: string
    manifest: ProjectManifest
    match?: (dependencyName: string) => boolean
    minimumReleaseAge?: number
    minimumReleaseAgeExclude?: string[]
    prefix: string
    publishedBy?: Date
    publishedByExclude?: PackageVersionPolicy
    wantedLockfile: LockfileObject | null
  }
): Promise<OutdatedPackage[]> {
  if (packageHasNoDeps(opts.manifest)) return []
  if (opts.wantedLockfile == null) {
    throw new PnpmError('OUTDATED_NO_LOCKFILE', `No lockfile in directory "${opts.lockfileDir}". Run \`pnpm install\` to generate one.`)
  }

  async function getOverriddenManifest () {
    const overrides = opts.currentLockfile?.overrides ?? opts.wantedLockfile?.overrides
    if (overrides) {
      const readPackageHook = createReadPackageHook({
        lockfileDir: opts.lockfileDir,
        overrides: parseOverrides(overrides, opts.catalogs ?? {}),
      })
      const manifest = await readPackageHook?.(opts.manifest, opts.lockfileDir)
      if (manifest) return manifest
    }

    return opts.manifest
  }

  const allDeps = getAllDependenciesFromManifest(await getOverriddenManifest())
  const importerId = getLockfileImporterId(opts.lockfileDir, opts.prefix)
  const currentLockfile: LockfileObject = opts.currentLockfile ?? { lockfileVersion: LOCKFILE_VERSION, importers: { [importerId]: { specifiers: {} } } }

  const outdated: OutdatedPackage[] = []

  const ignoreDependenciesMatcher = opts.ignoreDependencies?.length ? createMatcher(opts.ignoreDependencies) : undefined

  const resolveOpts = {
    lockfileDir: opts.lockfileDir,
    preferredVersions: {},
    projectDir: opts.prefix,
    publishedBy: opts.publishedBy,
    publishedByExclude: opts.publishedByExclude,
  }

  await Promise.all(
    DEPENDENCIES_FIELDS.map(async (depType) => {
      if (
        opts.include?.[depType] === false ||
        (opts.wantedLockfile!.importers[importerId][depType] == null)
      ) return

      let pkgs = Object.keys(opts.wantedLockfile!.importers[importerId][depType]!)

      if (opts.match != null) {
        pkgs = pkgs.filter((pkgName) => opts.match!(pkgName))
      }

      const _replaceCatalogProtocolIfNecessary = replaceCatalogProtocolIfNecessary.bind(null, opts.catalogs ?? {})

      await Promise.all(
        pkgs.map(async (alias) => {
          if (!allDeps[alias]) return
          const wantedRef = opts.wantedLockfile!.importers[importerId][depType]![alias]
          if (ignoreDependenciesMatcher?.(alias)) return

          const currentRef = (currentLockfile.importers[importerId] as ProjectSnapshot)?.[depType]?.[alias]
          const wantedRelative = dp.refToRelative(wantedRef, alias)
          const currentRelative = currentRef ? dp.refToRelative(currentRef, alias) : null
          const wantedSnapshot = wantedRelative != null ? opts.wantedLockfile!.packages?.[wantedRelative] : undefined
          const currentSnapshot = currentRelative != null ? currentLockfile.packages?.[currentRelative] : undefined
          // Aliased npm deps lock under their real name (e.g. `positive: is-positive@3.1.0`);
          // pull the name off the depPath so the report shows the real package.
          const packageName = (wantedRelative != null ? dp.parse(wantedRelative).name : undefined) ?? alias

          const bareSpecifier = _replaceCatalogProtocolIfNecessary({ alias, bareSpecifier: allDeps[alias] })

          const info = await opts.resolveLatest(
            { wantedDependency: { alias, bareSpecifier }, compatible: opts.compatible },
            resolveOpts
          )
          if (info == null) return // resolver doesn't claim this dep — skip silently

          const wanted = displayVersion(wantedRef, wantedRelative, wantedSnapshot?.version)
          const current = currentRef ? displayVersion(currentRef, currentRelative, currentSnapshot?.version) : undefined
          const { latestManifest } = info

          // Compare the parsed `wanted` / `current` rather than raw refs.
          // For npm-style deps that means peer-graph-only changes (same
          // semver, different `(peer-hash)`) don't surface as fake
          // "outdated" entries; for URL/git refs the display values *are*
          // the refs, so a commit/path change still fires correctly.
          if (latestManifest == null) {
            if (wanted !== current) {
              outdated.push({
                alias,
                belongsTo: depType,
                current,
                latestManifest: undefined,
                packageName,
                wanted,
                workspace: opts.manifest.name,
              })
            }
            return
          }
          if (!current) {
            outdated.push({
              alias,
              belongsTo: depType,
              latestManifest,
              packageName,
              wanted,
              workspace: opts.manifest.name,
            })
            return
          }
          if (wanted !== current || isLowerVersion(wanted, latestManifest.version) || latestManifest.deprecated) {
            outdated.push({
              alias,
              belongsTo: depType,
              current,
              latestManifest,
              packageName,
              wanted,
              workspace: opts.manifest.name,
            })
          }
        })
      )
    })
  )

  return outdated.sort((pkg1, pkg2) => pkg1.packageName.localeCompare(pkg2.packageName))
}

function packageHasNoDeps (manifest: ProjectManifest): boolean {
  return ((manifest.dependencies == null) || isEmpty(manifest.dependencies)) &&
    ((manifest.devDependencies == null) || isEmpty(manifest.devDependencies)) &&
    ((manifest.optionalDependencies == null) || isEmpty(manifest.optionalDependencies))
}

function isEmpty (obj: object): boolean {
  return Object.keys(obj).length === 0
}

// Pick a clean display string for a lockfile ref.
//
//   - If the dep-path parses to a semver, that's the value (handles
//     `pkg@1.0.0(peer-hash)` and aliased `positive: is-positive@3.1.0`).
//   - If the dep-path's non-semver version contains a `/`, it's a
//     URL/git-shape (`https://`, `git+ssh://`, scheme-less `github.com/…/sha`,
//     `link:../foo`, etc.) — return the raw ref so a commit/path change is
//     visible to the user.
//   - Otherwise prefer `snapshot.version` (clean semver for `runtime:`-style
//     refs); fall back to the raw ref when the snapshot didn't record one.
function displayVersion (ref: string, relativeDepPath: DepPath | null, snapshotVersion: string | undefined): string {
  if (relativeDepPath != null) {
    const parsed = dp.parse(relativeDepPath)
    if (parsed.version != null) return parsed.version
    if (parsed.nonSemverVersion?.includes('/')) return ref
  }
  return snapshotVersion ?? ref
}

// semver.lt throws on non-semver strings (e.g. URL refs from git/tarball).
// Treat those as "not lower" so a ref change still gets surfaced via the
// `wantedRef !== currentRef` check above.
function isLowerVersion (current: string, latest: string): boolean {
  if (!semver.valid(current) || !semver.valid(latest)) return false
  return semver.lt(current, latest)
}

function replaceCatalogProtocolIfNecessary (catalogs: Catalogs, wantedDependency: WantedDependency) {
  return matchCatalogResolveResult(resolveFromCatalog(catalogs, wantedDependency), {
    unused: () => wantedDependency.bareSpecifier,
    found: (found: CatalogResolutionFound) => found.resolution.specifier,
    misconfiguration: (misconfiguration) => {
      throw misconfiguration.error
    },
  })
}
