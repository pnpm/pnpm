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
    devDepPaths: R.unnest(R.values(lockfile.importers).map((deps) => resolvedDepsToRelDepPaths(deps.devDependencies || {}))),
    optionalDepPaths: R.unnest(R.values(lockfile.importers).map((deps) => resolvedDepsToRelDepPaths(deps.optionalDependencies || {}))),
    prodDepPaths: R.unnest(R.values(lockfile.importers).map((deps) => resolvedDepsToRelDepPaths(deps.dependencies || {}))),
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
    devDepPaths: string[],
    optionalDepPaths: string[],
    prodDepPaths: string[],
    warn: (msg: string) => void,
  }
): PackageSnapshots {
  const copiedPackages: PackageSnapshots = {}
  const nonOptional = new Set<string>()
  const notProdOnly = new Set<string>()
  const walked = new Set<string>()

  copyDependencySubGraph(copiedPackages, opts.devDepPaths, originalPackages, walked, opts.warn, {
    dev: true,
    nonOptional,
    notProdOnly,
    optional: false,
  })

  copyDependencySubGraph(copiedPackages, opts.optionalDepPaths, originalPackages, walked, opts.warn, {
    dev: false,
    nonOptional,
    notProdOnly,
    optional: true,
  })

  copyDependencySubGraph(copiedPackages, opts.prodDepPaths, originalPackages, walked, opts.warn, {
    dev: false,
    nonOptional,
    notProdOnly,
    optional: false,
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
  depPaths: string[],
  originalPackages: PackageSnapshots,
  walked: Set<string>,
  warn: (msg: string) => void,
  opts: {
    dev: boolean,
    optional: boolean,
    nonOptional: Set<string>,
    notProdOnly: Set<string>,
  }
) {
  for (const depPath of depPaths) {
    const key = `${depPath}:${opts.optional}:${opts.dev}`
    if (walked.has(key)) continue
    walked.add(key)
    if (!originalPackages[depPath]) {
      // local dependencies don't need to be resolved in pnpm-lock.yaml
      // except local tarball dependencies
      if (depPath.startsWith('link:') || depPath.startsWith('file:') && !depPath.endsWith('.tar.gz')) continue

      warn(`Cannot find resolution of ${depPath} in lockfile`)
      continue
    }
    const depLockfile = originalPackages[depPath]
    copiedSnapshots[depPath] = depLockfile
    if (opts.optional && !opts.nonOptional.has(depPath)) {
      depLockfile.optional = true
    } else {
      opts.nonOptional.add(depPath)
      delete depLockfile.optional
    }
    if (opts.dev) {
      opts.notProdOnly.add(depPath)
      depLockfile.dev = true
    } else if (depLockfile.dev === true) { // keeping if dev is explicitly false
      delete depLockfile.dev
    } else if (depLockfile.dev === undefined && !opts.notProdOnly.has(depPath)) {
      depLockfile.dev = false
    }
    const newDependencies = Object.entries(depLockfile.dependencies ?? {})
      .map(([alias, ref]) => refToRelative(ref, alias))
      .filter((depPath) => depPath !== null) as string[]
    copyDependencySubGraph(copiedSnapshots, newDependencies, originalPackages, walked, warn, opts)
    const newOptionalDependencies = Object.entries(depLockfile.optionalDependencies ?? {})
      .map(([alias, ref]) => refToRelative(ref, alias))
      .filter((depPath) => depPath !== null) as string[]
    copyDependencySubGraph(copiedSnapshots, newOptionalDependencies, originalPackages, walked, warn, { ...opts, optional: true })
  }
}
