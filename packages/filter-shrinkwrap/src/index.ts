import pnpmLogger from '@pnpm/logger'
import {
  PackageSnapshots,
  Shrinkwrap,
  ShrinkwrapImporter,
} from '@pnpm/shrinkwrap-types'
import { DependenciesField } from '@pnpm/types'
import * as dp from 'dependency-path'
import R = require('ramda')

const logger = pnpmLogger('shrinkwrap')

export function filterByImporters (
  shr: Shrinkwrap,
  importerIds: string[],
  opts: {
    defaultRegistry: string,
    include: { [dependenciesField in DependenciesField]: boolean },
    skipped: Set<string>,
    failOnMissingDependencies: boolean,
  },
): Shrinkwrap {
  if (R.equals(importerIds.sort(), R.keys(shr.importers).sort())) {
    return filterShrinkwrap(shr, opts)
  }
  const importerDeps = importerIds
    .map((importerId) => shr.importers[importerId])
    .map((importer) => ({
      ...(opts.include.dependencies && importer.dependencies || {}),
      ...(opts.include.devDependencies && importer.devDependencies || {}),
      ...(opts.include.optionalDependencies && importer.optionalDependencies || {}),
    }))
    .map(R.toPairs)
  const directDepPaths = R.unnest(importerDeps)
    .map(([pkgName, ref]) => dp.refToRelative(ref, pkgName))
    .filter((nodeId) => nodeId !== null) as string[]
  const packages = shr.packages &&
    pickPkgsWithAllDeps(shr.packages, directDepPaths, {
      failOnMissingDependencies: opts.failOnMissingDependencies,
      include: opts.include,
    }) || {}

  const importers = importerIds.reduce((acc, importerId) => {
    acc[importerId] = filterImporter(shr.importers[importerId], opts.include)
    return acc
  }, { ...shr.importers })

  return normalizeShrinkwrap({
    importers,
    packages,
    registry: shr.registry,
    shrinkwrapVersion: shr.shrinkwrapVersion,
  } as Shrinkwrap)
}

export default function filterShrinkwrap (
  shr: Shrinkwrap,
  opts: {
    defaultRegistry: string,
    include: { [dependenciesField in DependenciesField]: boolean },
    skipped: Set<string>,
  },
): Shrinkwrap {
  let pairs = R.toPairs(shr.packages || {})
    .filter(([relDepPath, pkg]) => !opts.skipped.has(pkg.id || dp.resolve(opts.defaultRegistry, relDepPath)))
  if (!opts.include.dependencies) {
    pairs = pairs.filter(([relDepPath, pkg]) => pkg.dev !== false || pkg.optional)
  }
  if (!opts.include.devDependencies) {
    pairs = pairs.filter(([relDepPath, pkg]) => pkg.dev !== true)
  }
  if (!opts.include.optionalDependencies) {
    pairs = pairs.filter(([relDepPath, pkg]) => !pkg.optional)
  }
  return normalizeShrinkwrap({
    importers: Object.keys(shr.importers).reduce((acc, importerId) => {
      acc[importerId] = filterImporter(shr.importers[importerId], opts.include)
      return acc
    }, {}),
    packages: R.fromPairs(pairs),
    registry: shr.registry,
    shrinkwrapVersion: shr.shrinkwrapVersion,
  } as Shrinkwrap)
}

function normalizeShrinkwrap (shr: Shrinkwrap) {
  if (!shr.registry) {
    delete shr.registry
  }
  return shr
}

function pickPkgsWithAllDeps (
  pkgSnapshots: PackageSnapshots,
  relDepPaths: string[],
  opts: {
    failOnMissingDependencies: boolean,
    include: { [dependenciesField in DependenciesField]: boolean },
  },
) {
  const pickedPackages = {} as PackageSnapshots
  pkgAllDeps(pkgSnapshots, pickedPackages, relDepPaths, opts)
  return pickedPackages
}

function pkgAllDeps (
  pkgSnapshots: PackageSnapshots,
  pickedPackages: PackageSnapshots,
  relDepPaths: string[],
  opts: {
    failOnMissingDependencies: boolean,
    include: { [dependenciesField in DependenciesField]: boolean },
  },
) {
  for (const relDepPath of relDepPaths) {
    if (pickedPackages[relDepPath]) continue
    const pkgSnapshot = pkgSnapshots[relDepPath]
    if (!pkgSnapshot && !relDepPath.startsWith('link:')) {
      const message = `No entry for "${relDepPath}" in shrinkwrap.yaml`
      if (opts.failOnMissingDependencies) {
        const err = new Error(message)
        err['code'] = 'ERR_PNPM_SHRINKWRAP_MISSING_DEPENDENCY' // tslint:disable-line:no-string-literal
        throw err
      }
      logger.debug(message)
      continue
    }
    pickedPackages[relDepPath] = pkgSnapshot
    const nextRelDepPaths = R.toPairs(
      {
        ...pkgSnapshot.dependencies,
        ...(opts.include.optionalDependencies && pkgSnapshot.optionalDependencies || {}),
      })
      .map(([pkgName, ref]) => dp.refToRelative(ref, pkgName))
      .filter((nodeId) => nodeId !== null) as string[]

    pkgAllDeps(pkgSnapshots, pickedPackages, nextRelDepPaths, opts)
  }
}

function filterImporter (
  importer: ShrinkwrapImporter,
  include: { [dependenciesField in DependenciesField]: boolean },
) {
  return {
    dependencies: !include.dependencies ? {} : importer.dependencies || {},
    devDependencies: !include.devDependencies ? {} : importer.devDependencies || {},
    optionalDependencies: !include.optionalDependencies ? {} : importer.optionalDependencies || {},
    specifiers: importer.specifiers,
  }
}
