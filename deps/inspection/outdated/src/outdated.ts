import {
  type CatalogResolutionFound,
  matchCatalogResolveResult,
  resolveFromCatalog,
  type WantedDependency,
} from '@pnpm/catalogs.resolver'
import type { Catalogs } from '@pnpm/catalogs.types'
import { createMatcher } from '@pnpm/config.matcher'
import { parseOverrides } from '@pnpm/config.parse-overrides'
import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import { LOCKFILE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import * as dp from '@pnpm/deps.path'
import { PnpmError } from '@pnpm/error'
import { createReadPackageHook } from '@pnpm/hooks.read-package-hook'
import type { OutdatedDispatcher } from '@pnpm/installing.client'
import {
  getLockfileImporterId,
  type LockfileObject,
  type ProjectSnapshot,
} from '@pnpm/lockfile.fs'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import { getAllDependenciesFromManifest } from '@pnpm/pkg-manifest.utils'
import {
  DEPENDENCIES_FIELDS,
  type DependenciesField,
  type IncludedDependencies,
  type PackageManifest,
  type PackageVersionPolicy,
  type ProjectManifest,
  type Registries,
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
    checkOutdated: OutdatedDispatcher
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
    registries: Registries
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
          const ref = opts.wantedLockfile!.importers[importerId][depType]![alias]
          if (ignoreDependenciesMatcher?.(alias)) return

          const relativeDepPath = dp.refToRelative(ref, alias)
          if (relativeDepPath === null) return // linked packages

          const pkgSnapshot = opts.wantedLockfile!.packages?.[relativeDepPath]
          if (pkgSnapshot == null) {
            throw new Error(`Invalid ${WANTED_LOCKFILE} file. ${relativeDepPath} not found in packages field`)
          }

          const currentRef = (currentLockfile.importers[importerId] as ProjectSnapshot)?.[depType]?.[alias]
          const currentRelative = currentRef && dp.refToRelative(currentRef, alias)
          const currentPkgSnapshot = currentRelative ? currentLockfile.packages?.[currentRelative] : undefined
          const wantedVersion = dp.parse(relativeDepPath).version ?? pkgSnapshot.version
          const currentVersion = (currentRelative && dp.parse(currentRelative).version) ?? currentPkgSnapshot?.version
          const { name: packageNameFromSnapshot } = nameVerFromPkgSnapshot(relativeDepPath, pkgSnapshot)
          const name = dp.parse(relativeDepPath).name ?? packageNameFromSnapshot

          const bareSpecifier = _replaceCatalogProtocolIfNecessary({ alias, bareSpecifier: allDeps[alias] })

          const info = await opts.checkOutdated(
            {
              wantedDependency: { alias, bareSpecifier },
              ref,
              currentRef,
              wantedVersion,
              currentVersion,
              compatible: opts.compatible,
              registry: pickRegistryForPackage(opts.registries, name),
            },
            resolveOpts
          )
          if (info == null) return
          const { packageName, current, wanted, latestManifest } = info

          if (latestManifest == null) {
            if (current !== wanted) {
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
          if (current !== wanted || isLowerVersion(current, latestManifest.version) || latestManifest.deprecated) {
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

// semver.lt throws on non-semver strings (e.g. when current is a URL because
// the resolver couldn't normalize it). Treat those as "not lower" so a ref
// change still gets surfaced via the current !== wanted branch above.
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
