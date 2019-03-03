import { SHRINKWRAP_VERSION } from '@pnpm/constants'
import {
  PackageSnapshots,
  ResolvedDependencies,
  Shrinkwrap,
  ShrinkwrapImporter,
} from '@pnpm/shrinkwrap-types'
import { PackageJson } from '@pnpm/types'
import { refToRelative } from 'dependency-path'
import R = require('ramda')

export * from '@pnpm/shrinkwrap-types'

export function pruneSharedShrinkwrap (
  shr: Shrinkwrap,
  opts: {
    defaultRegistry: string,
    warn?: (msg: string) => void,
  },
) {
  const copiedPackages = !shr.packages ? {} : copyPackageSnapshots(shr.packages, {
    devRelPaths: R.unnest(R.values(shr.importers).map((deps) => resolvedDepsToRelDepPaths(deps.devDependencies || {}))),
    optionalRelPaths: R.unnest(R.values(shr.importers).map((deps) => resolvedDepsToRelDepPaths(deps.optionalDependencies || {}))),
    prodRelPaths: R.unnest(R.values(shr.importers).map((deps) => resolvedDepsToRelDepPaths(deps.dependencies || {}))),
    registry: opts.defaultRegistry,
    warn: opts.warn || ((msg: string) => undefined),
  })

  const prunnedShr = {
    ...shr,
    packages: copiedPackages,
  }
  if (R.isEmpty(prunnedShr.packages)) {
    delete prunnedShr.packages
  }
  return prunnedShr
}

export function prune (
  shr: Shrinkwrap,
  pkg: PackageJson,
  importerId: string,
  opts: {
    defaultRegistry: string,
    warn?: (msg: string) => void,
  },
): Shrinkwrap {
  const packages: PackageSnapshots = {}
  const importer = shr.importers[importerId]
  const shrSpecs: ResolvedDependencies = importer.specifiers || {}
  const optionalDependencies = R.keys(pkg.optionalDependencies)
  const dependencies = R.difference(R.keys(pkg.dependencies), optionalDependencies)
  const devDependencies = R.difference(R.difference(R.keys(pkg.devDependencies), optionalDependencies), dependencies)
  const allDeps = R.reduce(R.union, [], [optionalDependencies, devDependencies, dependencies]) as string[]
  const specifiers: ResolvedDependencies = {}
  const shrDependencies: ResolvedDependencies = {}
  const shrOptionalDependencies: ResolvedDependencies = {}
  const shrDevDependencies: ResolvedDependencies = {}

  Object.keys(shrSpecs).forEach((depName) => {
    if (allDeps.indexOf(depName) === -1) return
    specifiers[depName] = shrSpecs[depName]
    if (importer.dependencies && importer.dependencies[depName]) {
      shrDependencies[depName] = importer.dependencies[depName]
    } else if (importer.optionalDependencies && importer.optionalDependencies[depName]) {
      shrOptionalDependencies[depName] = importer.optionalDependencies[depName]
    } else if (importer.devDependencies && importer.devDependencies[depName]) {
      shrDevDependencies[depName] = importer.devDependencies[depName]
    }
  })
  if (importer.dependencies) {
    for (const dep of R.keys(importer.dependencies)) {
      if (
        !shrDependencies[dep] && importer.dependencies[dep].startsWith('link:') &&
        // If the linked dependency was removed from package.json
        // then it is removed from shrinkwrap.yaml as well
        !(shrSpecs[dep] && !allDeps[dep])
      ) {
        shrDependencies[dep] = importer.dependencies[dep]
      }
    }
  }

  const updatedImporter: ShrinkwrapImporter = {
    specifiers,
  }
  const prunnedShrinkwrap: Shrinkwrap = {
    importers: {
      ...shr.importers,
      [importerId]: updatedImporter,
    },
    lockfileVersion: shr.lockfileVersion || SHRINKWRAP_VERSION,
    packages: shr.packages,
  }
  if (!R.isEmpty(packages)) {
    prunnedShrinkwrap.packages = packages
  }
  if (!R.isEmpty(shrDependencies)) {
    updatedImporter.dependencies = shrDependencies
  }
  if (!R.isEmpty(shrOptionalDependencies)) {
    updatedImporter.optionalDependencies = shrOptionalDependencies
  }
  if (!R.isEmpty(shrDevDependencies)) {
    updatedImporter.devDependencies = shrDevDependencies
  }
  return pruneSharedShrinkwrap(prunnedShrinkwrap, opts)
}

function copyPackageSnapshots (
  originalPackages: PackageSnapshots,
  opts: {
    devRelPaths: string[],
    optionalRelPaths: string[],
    prodRelPaths: string[],
    registry: string,
    warn: (msg: string) => void,
  },
): PackageSnapshots {
  const copiedPackages: PackageSnapshots = {}
  const nonOptional = new Set()
  const notProdOnly = new Set()

  copyDependencySubGraph(copiedPackages, opts.devRelPaths, originalPackages, new Set(), opts.warn, {
    dev: true,
    nonOptional,
    notProdOnly,
    registry: opts.registry,
  })

  copyDependencySubGraph(copiedPackages, opts.prodRelPaths, originalPackages, new Set(), opts.warn, {
    nonOptional,
    notProdOnly,
    registry: opts.registry,
  })

  copyDependencySubGraph(copiedPackages, opts.optionalRelPaths, originalPackages, new Set(), opts.warn, {
    nonOptional,
    notProdOnly,
    optional: true,
    registry: opts.registry,
  })

  copyDependencySubGraph(copiedPackages, opts.devRelPaths, originalPackages, new Set(), opts.warn, {
    dev: true,
    nonOptional,
    notProdOnly,
    registry: opts.registry,
    walkOptionals: true,
  })

  copyDependencySubGraph(copiedPackages, opts.prodRelPaths, originalPackages, new Set(), opts.warn, {
    nonOptional,
    notProdOnly,
    registry: opts.registry,
    walkOptionals: true,
  })

  return copiedPackages
}

function resolvedDepsToRelDepPaths (deps: ResolvedDependencies) {
  return R.keys(deps)
    .map((pkgName: string) => refToRelative(deps[pkgName], pkgName))
    .filter((relPath) => relPath !== null) as string[]
}

function copyDependencySubGraph (
  copiedSnapshots: PackageSnapshots,
  depRelativePaths: string[],
  originalPackages: PackageSnapshots,
  walked: Set<string>,
  warn: (msg: string) => void,
  opts: {
    registry: string,
    dev?: boolean,
    optional?: boolean,
    nonOptional: Set<string>,
    notProdOnly: Set<string>,
    walkOptionals?: boolean,
  },
) {
  for (const depRalativePath of depRelativePaths) {
    if (walked.has(depRalativePath)) continue
    walked.add(depRalativePath)
    if (!originalPackages[depRalativePath]) {
      // local dependencies don't need to be resolved in shrinkwrap.yaml
      // except local tarball dependencies
      if (depRalativePath.startsWith('link:') || depRalativePath.startsWith('file:') && !depRalativePath.endsWith('.tar.gz')) continue

      // NOTE: Warnings should not be printed for the current shrinkwrap file (node_modules/.shrinkwrap.yaml).
      // The current shrinkwrap file does not contain the skipped packages, so it may have missing resolutions
      warn(`Cannot find resolution of ${depRalativePath} in shrinkwrap file`)
      continue
    }
    const depShr = originalPackages[depRalativePath]
    copiedSnapshots[depRalativePath] = depShr
    if (opts.optional && !opts.nonOptional.has(depRalativePath)) {
      depShr.optional = true
    } else {
      opts.nonOptional.add(depRalativePath)
      delete depShr.optional
    }
    if (opts.dev) {
      opts.notProdOnly.add(depRalativePath)
      depShr.dev = true
    } else if (depShr.dev === true) { // keeping if dev is explicitly false
      delete depShr.dev
    } else if (depShr.dev === undefined && !opts.notProdOnly.has(depRalativePath)) {
      depShr.dev = false
    }
    const newDependencies = R.keys(depShr.dependencies)
      .map((pkgName: string) => refToRelative((depShr.dependencies && depShr.dependencies[pkgName]) as string, pkgName))
      .filter((relPath) => relPath !== null) as string[]
    copyDependencySubGraph(copiedSnapshots, newDependencies, originalPackages, walked, warn, opts)
    if (!opts.walkOptionals) continue
    const newOptionalDependencies = R.keys(depShr.optionalDependencies)
      .map((pkgName: string) => refToRelative((depShr.optionalDependencies && depShr.optionalDependencies[pkgName]) as string, pkgName))
      .filter((relPath) => relPath !== null) as string[]
    copyDependencySubGraph(copiedSnapshots, newOptionalDependencies, originalPackages, walked, warn, { ...opts, optional: true })
  }
}
