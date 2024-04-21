import { WANTED_LOCKFILE } from '@pnpm/constants'
import { LockfileMissingDependencyError } from '@pnpm/error'
import {
  type Lockfile,
  type PackageSnapshots,
} from '@pnpm/lockfile-types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile-utils'
import { logger } from '@pnpm/logger'
import { packageIsInstallable } from '@pnpm/package-is-installable'
import { type SupportedArchitectures, type DependenciesField } from '@pnpm/types'
import * as dp from '@pnpm/dependency-path'
import mapValues from 'ramda/src/map'
import pickBy from 'ramda/src/pickBy'
import unnest from 'ramda/src/unnest'
import { filterImporter } from './filterImporter'

const lockfileLogger = logger('lockfile')

export interface FilterLockfileResult {
  lockfile: Lockfile
  selectedImporterIds: string[]
}

export function filterLockfileByEngine (
  lockfile: Lockfile,
  opts: FilterLockfileOptions
): FilterLockfileResult {
  const importerIds = Object.keys(lockfile.importers)
  return filterLockfileByImportersAndEngine(lockfile, importerIds, opts)
}

export interface FilterLockfileOptions {
  currentEngine: {
    nodeVersion?: string
    pnpmVersion: string
  }
  engineStrict: boolean
  include: { [dependenciesField in DependenciesField]: boolean }
  includeIncompatiblePackages?: boolean
  failOnMissingDependencies: boolean
  lockfileDir: string
  skipped: Set<string>
  supportedArchitectures?: SupportedArchitectures
}

export function filterLockfileByImportersAndEngine (
  lockfile: Lockfile,
  importerIds: string[],
  opts: FilterLockfileOptions
): FilterLockfileResult {
  const importerIdSet = new Set(importerIds) as Set<string>

  const directDepPaths = toImporterDepPaths(lockfile, importerIds, {
    include: opts.include,
    importerIdSet,
  })

  const packages =
    lockfile.packages != null
      ? pickPkgsWithAllDeps(lockfile, directDepPaths, importerIdSet, {
        currentEngine: opts.currentEngine,
        engineStrict: opts.engineStrict,
        failOnMissingDependencies: opts.failOnMissingDependencies,
        include: opts.include,
        includeIncompatiblePackages:
            opts.includeIncompatiblePackages === true,
        lockfileDir: opts.lockfileDir,
        skipped: opts.skipped,
        supportedArchitectures: opts.supportedArchitectures,
      })
      : {}

  const importers = mapValues((importer) => {
    const newImporter = filterImporter(importer, opts.include)
    if (newImporter.optionalDependencies != null) {
      newImporter.optionalDependencies = pickBy((ref, depName) => {
        const depPath = dp.refToRelative(ref, depName)
        return !depPath || packages[depPath] != null
      }, newImporter.optionalDependencies)
    }
    return newImporter
  }, lockfile.importers)

  return {
    lockfile: {
      ...lockfile,
      importers,
      packages,
    },
    selectedImporterIds: Array.from(importerIdSet),
  }
}

function pickPkgsWithAllDeps (
  lockfile: Lockfile,
  depPaths: string[],
  importerIdSet: Set<string>,
  opts: {
    currentEngine: {
      nodeVersion?: string
      pnpmVersion: string
    }
    engineStrict: boolean
    failOnMissingDependencies: boolean
    include: { [dependenciesField in DependenciesField]: boolean }
    includeIncompatiblePackages: boolean
    lockfileDir: string
    skipped: Set<string>
    supportedArchitectures?: SupportedArchitectures
  }
): PackageSnapshots {
  const pickedPackages = {} as PackageSnapshots
  pkgAllDeps({ lockfile, pickedPackages, importerIdSet }, depPaths, true, opts)
  return pickedPackages
}

function pkgAllDeps (
  ctx: {
    lockfile: Lockfile
    pickedPackages: PackageSnapshots
    importerIdSet: Set<string>
  },
  depPaths: string[],
  parentIsInstallable: boolean,
  opts: {
    currentEngine: {
      nodeVersion?: string
      pnpmVersion: string
    }
    engineStrict: boolean
    failOnMissingDependencies: boolean
    include: { [dependenciesField in DependenciesField]: boolean }
    includeIncompatiblePackages: boolean
    lockfileDir: string
    skipped: Set<string>
    supportedArchitectures?: SupportedArchitectures
  }
) {
  for (const depPath of depPaths) {
    if (ctx.pickedPackages[depPath]) continue
    const pkgSnapshot = ctx.lockfile.packages![depPath]
    if (!pkgSnapshot && !depPath.startsWith('link:')) {
      if (opts.failOnMissingDependencies) {
        throw new LockfileMissingDependencyError(depPath)
      }
      lockfileLogger.debug(`No entry for "${depPath}" in ${WANTED_LOCKFILE}`)
      continue
    }
    let installable!: boolean
    if (!parentIsInstallable) {
      installable = false
      if (!ctx.pickedPackages[depPath] && pkgSnapshot.optional === true) {
        opts.skipped.add(depPath)
      }
    } else {
      const pkg = {
        ...nameVerFromPkgSnapshot(depPath, pkgSnapshot),
        cpu: pkgSnapshot.cpu,
        engines: pkgSnapshot.engines,
        os: pkgSnapshot.os,
        libc: pkgSnapshot.libc,
      }
      // TODO: depPath is not the package ID. Should be fixed
      installable =
        opts.includeIncompatiblePackages ||
        packageIsInstallable(pkgSnapshot.id ?? depPath, pkg, {
          engineStrict: opts.engineStrict,
          lockfileDir: opts.lockfileDir,
          nodeVersion: opts.currentEngine.nodeVersion,
          optional: pkgSnapshot.optional === true,
          supportedArchitectures: opts.supportedArchitectures,
        }) !== false
      if (!installable) {
        if (!ctx.pickedPackages[depPath] && pkgSnapshot.optional === true) {
          opts.skipped.add(depPath)
        }
      } else {
        opts.skipped.delete(depPath)
      }
    }
    ctx.pickedPackages[depPath] = pkgSnapshot
    const { depPaths: nextRelDepPaths, importerIds: additionalImporterIds } = parseDepRefs(Object.entries({
      ...pkgSnapshot.dependencies,
      ...(opts.include.optionalDependencies
        ? pkgSnapshot.optionalDependencies
        : {}),
    }), ctx.lockfile)
    additionalImporterIds.forEach((importerId) => ctx.importerIdSet.add(importerId))
    nextRelDepPaths.push(
      ...toImporterDepPaths(ctx.lockfile, additionalImporterIds, {
        include: opts.include,
        importerIdSet: ctx.importerIdSet,
      })
    )
    pkgAllDeps(ctx, nextRelDepPaths, installable, opts)
  }
}

function toImporterDepPaths (
  lockfile: Lockfile,
  importerIds: string[],
  opts: {
    include: { [dependenciesField in DependenciesField]: boolean }
    importerIdSet: Set<string>
  }
): string[] {
  const importerDeps = importerIds
    .map(importerId => lockfile.importers[importerId])
    .map(importer => ({
      ...(opts.include.dependencies ? importer.dependencies : {}),
      ...(opts.include.devDependencies ? importer.devDependencies : {}),
      ...(opts.include.optionalDependencies
        ? importer.optionalDependencies
        : {}),
    }))
    .map(Object.entries)

  const { depPaths, importerIds: nextImporterIds } = parseDepRefs(unnest(importerDeps), lockfile)

  if (!nextImporterIds.length) {
    return depPaths
  }
  nextImporterIds.forEach((importerId) => {
    opts.importerIdSet.add(importerId)
  })
  return [
    ...depPaths,
    ...toImporterDepPaths(lockfile, nextImporterIds, opts),
  ]
}

interface ParsedDepRefs {
  depPaths: string[]
  importerIds: string[]
}

function parseDepRefs (refsByPkgNames: Array<[string, string]>, lockfile: Lockfile): ParsedDepRefs {
  return refsByPkgNames
    .reduce((acc, [pkgName, ref]) => {
      if (ref.startsWith('link:')) {
        const importerId = ref.substring(5)
        if (lockfile.importers[importerId]) {
          acc.importerIds.push(importerId)
        }
        return acc
      }
      const depPath = dp.refToRelative(ref, pkgName)
      if (depPath == null) return acc
      acc.depPaths.push(depPath)
      return acc
    }, { depPaths: [], importerIds: [] } as ParsedDepRefs)
}
