import { LOCKFILE_VERSION } from '@pnpm/constants'
import {
  Lockfile,
  PackageSnapshots,
  ProjectSnapshot,
  ResolvedDependencies,
} from '@pnpm/lockfile-types'
import { PackageManifest } from '@pnpm/types'
import { refToRelative } from 'dependency-path'
import R = require('ramda')

export * from '@pnpm/lockfile-types'

export function pruneSharedLockfile (
  lockfile: Lockfile,
  opts?: {
    warn?: (msg: string) => void,
  }
) {
  const copiedPackages = !lockfile.packages ? {} : copyPackageSnapshots(lockfile.packages, {
    devRelPaths: R.unnest(R.values(lockfile.importers).map((deps) => resolvedDepsToRelDepPaths(deps.devDependencies || {}))),
    optionalRelPaths: R.unnest(R.values(lockfile.importers).map((deps) => resolvedDepsToRelDepPaths(deps.optionalDependencies || {}))),
    prodRelPaths: R.unnest(R.values(lockfile.importers).map((deps) => resolvedDepsToRelDepPaths(deps.dependencies || {}))),
    warn: opts && opts.warn || ((msg: string) => undefined),
  })

  const prunnedLockfile = {
    ...lockfile,
    packages: copiedPackages,
  }
  if (R.isEmpty(prunnedLockfile.packages)) {
    delete prunnedLockfile.packages
  }
  return prunnedLockfile
}

export function pruneLockfile (
  lockfile: Lockfile,
  pkg: PackageManifest,
  importerId: string,
  opts?: {
    warn?: (msg: string) => void,
  }
): Lockfile {
  const packages: PackageSnapshots = {}
  const importer = lockfile.importers[importerId]
  const lockfileSpecs: ResolvedDependencies = importer.specifiers || {}
  const optionalDependencies = R.keys(pkg.optionalDependencies)
  const dependencies = R.difference(R.keys(pkg.dependencies), optionalDependencies)
  const devDependencies = R.difference(R.difference(R.keys(pkg.devDependencies), optionalDependencies), dependencies)
  const allDeps = [
    ...optionalDependencies,
    ...devDependencies,
    ...dependencies,
  ]
  const specifiers: ResolvedDependencies = {}
  const lockfileDependencies: ResolvedDependencies = {}
  const lockfileOptionalDependencies: ResolvedDependencies = {}
  const lockfileDevDependencies: ResolvedDependencies = {}

  Object.keys(lockfileSpecs).forEach((depName) => {
    if (!allDeps.includes(depName)) return
    specifiers[depName] = lockfileSpecs[depName]
    if (importer.dependencies && importer.dependencies[depName]) {
      lockfileDependencies[depName] = importer.dependencies[depName]
    } else if (importer.optionalDependencies && importer.optionalDependencies[depName]) {
      lockfileOptionalDependencies[depName] = importer.optionalDependencies[depName]
    } else if (importer.devDependencies && importer.devDependencies[depName]) {
      lockfileDevDependencies[depName] = importer.devDependencies[depName]
    }
  })
  if (importer.dependencies) {
    for (const dep of R.keys(importer.dependencies)) {
      if (
        !lockfileDependencies[dep] && importer.dependencies[dep].startsWith('link:') &&
        // If the linked dependency was removed from package.json
        // then it is removed from pnpm-lock.yaml as well
        !(lockfileSpecs[dep] && !allDeps[dep])
      ) {
        lockfileDependencies[dep] = importer.dependencies[dep]
      }
    }
  }

  const updatedImporter: ProjectSnapshot = {
    specifiers,
  }
  const prunnedLockfile: Lockfile = {
    importers: {
      ...lockfile.importers,
      [importerId]: updatedImporter,
    },
    lockfileVersion: lockfile.lockfileVersion || LOCKFILE_VERSION,
    packages: lockfile.packages,
  }
  if (!R.isEmpty(packages)) {
    prunnedLockfile.packages = packages
  }
  if (!R.isEmpty(lockfileDependencies)) {
    updatedImporter.dependencies = lockfileDependencies
  }
  if (!R.isEmpty(lockfileOptionalDependencies)) {
    updatedImporter.optionalDependencies = lockfileOptionalDependencies
  }
  if (!R.isEmpty(lockfileDevDependencies)) {
    updatedImporter.devDependencies = lockfileDevDependencies
  }
  return pruneSharedLockfile(prunnedLockfile, opts)
}

function copyPackageSnapshots (
  originalPackages: PackageSnapshots,
  opts: {
    devRelPaths: string[],
    optionalRelPaths: string[],
    prodRelPaths: string[],
    warn: (msg: string) => void,
  }
): PackageSnapshots {
  const copiedPackages: PackageSnapshots = {}
  const nonOptional = new Set<string>()
  const notProdOnly = new Set<string>()

  copyDependencySubGraph(copiedPackages, opts.devRelPaths, originalPackages, new Set(), opts.warn, {
    dev: true,
    nonOptional,
    notProdOnly,
  })

  copyDependencySubGraph(copiedPackages, opts.prodRelPaths, originalPackages, new Set(), opts.warn, {
    nonOptional,
    notProdOnly,
  })

  copyDependencySubGraph(copiedPackages, opts.optionalRelPaths, originalPackages, new Set(), opts.warn, {
    nonOptional,
    notProdOnly,
    optional: true,
  })

  copyDependencySubGraph(copiedPackages, opts.devRelPaths, originalPackages, new Set(), opts.warn, {
    dev: true,
    nonOptional,
    notProdOnly,
    walkOptionals: true,
  })

  copyDependencySubGraph(copiedPackages, opts.prodRelPaths, originalPackages, new Set(), opts.warn, {
    nonOptional,
    notProdOnly,
    walkOptionals: true,
  })

  return copiedPackages
}

function resolvedDepsToRelDepPaths (deps: ResolvedDependencies) {
  return Object.keys(deps)
    .map((pkgName) => refToRelative(deps[pkgName], pkgName))
    .filter((relPath) => relPath !== null) as string[]
}

function copyDependencySubGraph (
  copiedSnapshots: PackageSnapshots,
  depRelativePaths: string[],
  originalPackages: PackageSnapshots,
  walked: Set<string>,
  warn: (msg: string) => void,
  opts: {
    dev?: boolean,
    optional?: boolean,
    nonOptional: Set<string>,
    notProdOnly: Set<string>,
    walkOptionals?: boolean,
  }
) {
  for (const depRalativePath of depRelativePaths) {
    if (walked.has(depRalativePath)) continue
    walked.add(depRalativePath)
    if (!originalPackages[depRalativePath]) {
      // local dependencies don't need to be resolved in pnpm-lock.yaml
      // except local tarball dependencies
      if (depRalativePath.startsWith('link:') || depRalativePath.startsWith('file:') && !depRalativePath.endsWith('.tar.gz')) continue

      // NOTE: Warnings should not be printed for the current lockfile (node_modules/.lockfile.yaml).
      // The current lockfile does not contain the skipped packages, so it may have missing resolutions
      warn(`Cannot find resolution of ${depRalativePath} in lockfile`)
      continue
    }
    const depLockfile = originalPackages[depRalativePath]
    copiedSnapshots[depRalativePath] = depLockfile
    if (opts.optional && !opts.nonOptional.has(depRalativePath)) {
      depLockfile.optional = true
    } else {
      opts.nonOptional.add(depRalativePath)
      delete depLockfile.optional
    }
    if (opts.dev) {
      opts.notProdOnly.add(depRalativePath)
      depLockfile.dev = true
    } else if (depLockfile.dev === true) { // keeping if dev is explicitly false
      delete depLockfile.dev
    } else if (depLockfile.dev === undefined && !opts.notProdOnly.has(depRalativePath)) {
      depLockfile.dev = false
    }
    const newDependencies = R.keys(depLockfile.dependencies)
      .map((pkgName: string) => refToRelative((depLockfile.dependencies && depLockfile.dependencies[pkgName]) as string, pkgName))
      .filter((relPath) => relPath !== null) as string[]
    copyDependencySubGraph(copiedSnapshots, newDependencies, originalPackages, walked, warn, opts)
    if (!opts.walkOptionals) continue
    const newOptionalDependencies = R.keys(depLockfile.optionalDependencies)
      .map((pkgName: string) => refToRelative((depLockfile.optionalDependencies && depLockfile.optionalDependencies[pkgName]) as string, pkgName))
      .filter((relPath) => relPath !== null) as string[]
    copyDependencySubGraph(copiedSnapshots, newOptionalDependencies, originalPackages, walked, warn, { ...opts, optional: true })
  }
}
