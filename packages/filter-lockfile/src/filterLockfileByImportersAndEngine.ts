import { WANTED_LOCKFILE } from '@pnpm/constants'
import { LockfileMissingDependencyError } from '@pnpm/error'
import {
  Lockfile,
  PackageSnapshots,
} from '@pnpm/lockfile-types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile-utils'
import pnpmLogger from '@pnpm/logger'
import packageIsInstallable from '@pnpm/package-is-installable'
import { DependenciesField } from '@pnpm/types'
import * as dp from 'dependency-path'
import unnest from 'ramda/src/unnest'
import filterImporter from './filterImporter'

const logger = pnpmLogger('lockfile')

function toImporterDepPaths (
  lockfile: Lockfile,
  importerIds: string[],
  opts: {
    include: { [dependenciesField in DependenciesField]: boolean }
  }
) {
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

  const importerDepsPaths = unnest(importerDeps)
    .map(([pkgName, ref]) => dp.refToRelative(ref, pkgName))
    .filter(nodeId => nodeId !== null) as string[]

  return importerDepsPaths
}

export default function filterByImportersAndEngine (
  lockfile: Lockfile,
  importerIds: string[],
  opts: {
    currentEngine: {
      nodeVersion: string
      pnpmVersion: string
    }
    engineStrict: boolean
    include: { [dependenciesField in DependenciesField]: boolean }
    includeIncompatiblePackages?: boolean
    failOnMissingDependencies: boolean
    lockfileDir: string
    skipped: Set<string>
  }
): { lockfile: Lockfile, importerIds: string[] } {
  const importerIdSet = new Set(importerIds) as Set<string>

  const directDepPaths = toImporterDepPaths(lockfile, importerIds, {
    include: opts.include,
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
      })
      : {}

  const importers = importerIds.reduce((acc, importerId) => {
    acc[importerId] = filterImporter(lockfile.importers[importerId], opts.include)
    if (acc[importerId].optionalDependencies != null) {
      for (const depName of Object.keys(acc[importerId].optionalDependencies ?? {})) {
        const depPath = dp.refToRelative(acc[importerId].optionalDependencies![depName], depName)
        if (depPath && !packages[depPath]) {
          delete acc[importerId].optionalDependencies![depName]
        }
      }
    }
    return acc
  }, { ...lockfile.importers })

  return {
    lockfile: {
      ...lockfile,
      importers,
      packages,
    },
    importerIds: Array.from(importerIdSet),
  }
}

function pickPkgsWithAllDeps (
  lockfile: Lockfile,
  depPaths: string[],
  importerIdSet: Set<string>,
  opts: {
    currentEngine: {
      nodeVersion: string
      pnpmVersion: string
    }
    engineStrict: boolean
    failOnMissingDependencies: boolean
    include: { [dependenciesField in DependenciesField]: boolean }
    includeIncompatiblePackages: boolean
    lockfileDir: string
    skipped: Set<string>
  }
) {
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
      nodeVersion: string
      pnpmVersion: string
    }
    engineStrict: boolean
    failOnMissingDependencies: boolean
    include: { [dependenciesField in DependenciesField]: boolean }
    includeIncompatiblePackages: boolean
    lockfileDir: string
    skipped: Set<string>
  }
) {
  for (const depPath of depPaths) {
    if (ctx.pickedPackages[depPath]) continue
    const pkgSnapshot = ctx.lockfile.packages![depPath]
    if (!pkgSnapshot && !depPath.startsWith('link:')) {
      if (opts.failOnMissingDependencies) {
        throw new LockfileMissingDependencyError(depPath)
      }
      logger.debug(`No entry for "${depPath}" in ${WANTED_LOCKFILE}`)
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
          pnpmVersion: opts.currentEngine.pnpmVersion,
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
    const nextRelDepPaths = Object.entries({
      ...pkgSnapshot.dependencies,
      ...(opts.include.optionalDependencies
        ? pkgSnapshot.optionalDependencies
        : {}),
    })
      .map(([pkgName, ref]) => {
        if (ref.startsWith('link:')) {
          return ref
        }
        return dp.refToRelative(ref, pkgName)
      })
      .filter(nodeId => nodeId !== null) as string[]

    // Also include missing deeply linked workspace project
    const actualNextRelDepPaths = []
    const additionalImporterIds = []
    for (const nextDepPath of nextRelDepPaths) {
      if (nextDepPath.startsWith('link:')) {
        const ref = nextDepPath.slice(5)
        additionalImporterIds.push(ref)
        ctx.importerIdSet.add(ref)
      } else {
        actualNextRelDepPaths.push(nextDepPath)
      }
    }

    actualNextRelDepPaths.push(
      ...toImporterDepPaths(ctx.lockfile, additionalImporterIds, {
        include: opts.include,
      })
    )

    pkgAllDeps(ctx, actualNextRelDepPaths, installable, opts)
  }
}
