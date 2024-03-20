import semver from 'semver'

import {
  getLockfileImporterId,
} from '@pnpm/lockfile-file'
import { PnpmError } from '@pnpm/error'
import { parsePref } from '@pnpm/npm-resolver'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile-utils'
import { getAllDependenciesFromManifest } from '@pnpm/manifest-utils'
import { pickRegistryForPackage } from '@pnpm/pick-registry-for-package'
import {
  type Lockfile,
  type Registries,
  DEPENDENCIES_FIELDS,
  type ProjectSnapshot,
  type OutdatedPackage,
  type ProjectManifest,
  type IncludedDependencies,
  type GetLatestManifestFunction,
} from '@pnpm/types'
import * as dp from '@pnpm/dependency-path'
import { createMatcher } from '@pnpm/matcher'
import { createReadPackageHook } from '@pnpm/hooks.read-package-hook'

export * from './createManifestGetter'

export async function outdated(opts: {
  compatible?: boolean | undefined
  currentLockfile: Lockfile | null
  getLatestManifest: GetLatestManifestFunction
  ignoreDependencies?: string[] | undefined
  include?: IncludedDependencies | undefined
  lockfileDir: string
  manifest: ProjectManifest
  match?: ((dependencyName: string) => boolean) | undefined
  prefix: string
  registries: Registries
  wantedLockfile: Lockfile | null
}): Promise<OutdatedPackage[]> {
  if (packageHasNoDeps(opts.manifest)) {
    return []
  }

  if (opts.wantedLockfile == null) {
    throw new PnpmError(
      'OUTDATED_NO_LOCKFILE',
      `No lockfile in directory "${opts.lockfileDir}". Run \`pnpm install\` to generate one.`
    )
  }

  async function getOverriddenManifest() {
    const overrides =
      opts.currentLockfile?.overrides ?? opts.wantedLockfile?.overrides

    if (overrides) {
      const readPackageHook = createReadPackageHook({
        lockfileDir: opts.lockfileDir,
        overrides,
      })

      const manifest = await readPackageHook?.(opts.manifest, opts.lockfileDir)

      if (manifest) {
        return manifest
      }
    }

    return opts.manifest
  }

  const allDeps = getAllDependenciesFromManifest(await getOverriddenManifest())

  const importerId = getLockfileImporterId(opts.lockfileDir, opts.prefix)

  const currentLockfile = opts.currentLockfile ?? {
    importers: { [importerId]: {} },
  }

  const outdated: OutdatedPackage[] = []

  const ignoreDependenciesMatcher = opts.ignoreDependencies?.length
    ? createMatcher(opts.ignoreDependencies)
    : undefined

  await Promise.all(
    DEPENDENCIES_FIELDS.map(async (depType: 'optionalDependencies' | 'dependencies' | 'devDependencies'): Promise<void> => {
      if (
        opts.include?.[depType] === false ||
        opts.wantedLockfile?.importers[importerId][depType] == null
      )
        return

      let pkgs = Object.keys(
        opts.wantedLockfile?.importers[importerId][depType]!
      )

      if (opts.match != null) {
        pkgs = pkgs.filter((pkgName) => opts.match!(pkgName))
      }

      await Promise.all(
        pkgs.map(async (alias: string): Promise<void> => {
          if (!allDeps[alias]) {
            return
          }

          const ref =
            opts.wantedLockfile?.importers[importerId][depType]?.[alias]

          if (
            ref?.startsWith('file:') || // ignoring linked packages. (For backward compatibility)
            ignoreDependenciesMatcher?.(alias)
          ) {
            return
          }

          const relativeDepPath = dp.refToRelative(ref ?? '', alias)

          // ignoring linked packages
          if (relativeDepPath === null) {
            return
          }

          const pkgSnapshot = opts.wantedLockfile?.packages?.[relativeDepPath]

          if (pkgSnapshot == null) {
            throw new Error(
              `Invalid ${WANTED_LOCKFILE} file. ${relativeDepPath} not found in packages field`
            )
          }

          const currentRef = (
            currentLockfile.importers[importerId] as ProjectSnapshot
          )?.[depType]?.[alias]

          const currentRelative =
            currentRef && dp.refToRelative(currentRef, alias)

          const parsed = dp.parse(relativeDepPath)

          const current =
            (currentRelative && 'version' in parsed ? parsed.version : undefined) ?? currentRef

          const wanted = 'version' in parsed ? parsed.version : ref ?? ''

          const { name: packageName } = nameVerFromPkgSnapshot(
            relativeDepPath,
            pkgSnapshot
          )

          const name = 'name' in parsed ? parsed.name : packageName

          // If the npm resolve parser cannot parse the spec of the dependency,
          // it means that the package is not from a npm-compatible registry.
          // In that case, we can't check whether the package is up-to-date
          if (
            parsePref(
              allDeps[alias],
              alias,
              'latest',
              pickRegistryForPackage(opts.registries, name)
            ) == null
          ) {
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

          const latestManifest = await opts.getLatestManifest(
            name,
            opts.compatible ? allDeps[name] ?? 'latest' : 'latest'
          )

          if (latestManifest == null) return

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

          if (
            current !== wanted ||
            semver.lt(current, latestManifest.version) ||
            latestManifest.deprecated
          ) {
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

  return outdated.sort((pkg1: OutdatedPackage, pkg2: OutdatedPackage): number => {
    return pkg1.packageName.localeCompare(pkg2.packageName);
  }
  )
}

function packageHasNoDeps(manifest: ProjectManifest): boolean {
  return (
    (manifest.dependencies == null || isEmpty(manifest.dependencies)) &&
    (manifest.devDependencies == null || isEmpty(manifest.devDependencies)) &&
    (manifest.optionalDependencies == null ||
      isEmpty(manifest.optionalDependencies))
  )
}

function isEmpty(obj: object): boolean {
  return Object.keys(obj).length === 0
}
