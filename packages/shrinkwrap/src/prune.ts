import logger from '@pnpm/logger'
import {PackageJson} from '@pnpm/types'
import {refToRelative} from 'dependency-path'
import R = require('ramda')
import {SHRINKWRAP_VERSION} from './constants'
import {
  ResolvedDependencies,
  ResolvedPackages,
  Shrinkwrap,
} from './types'

export function pruneWithoutPackageJson (shr: Shrinkwrap, warn: (msg: string) => void) {
  return _prune(shr, undefined, warn)
}

export function prune (shr: Shrinkwrap, pkg: PackageJson, warn: (msg: string) => void) {
  return _prune(shr, pkg, warn)
}

function _prune (
  shr: Shrinkwrap,
  pkg: PackageJson | undefined,
  warn: (msg: string) => void,
): Shrinkwrap {
  const packages: ResolvedPackages = {}
  const shrSpecs: ResolvedDependencies = shr.specifiers || {}
  let allDeps!: string[]
  if (pkg) {
    const optionalDependencies = R.keys(pkg.optionalDependencies)
    const dependencies = R.difference(R.keys(pkg.dependencies), optionalDependencies)
    const devDependencies = R.difference(R.difference(R.keys(pkg.devDependencies), optionalDependencies), dependencies)
    allDeps = R.reduce(R.union, [], [optionalDependencies, devDependencies, dependencies]) as string[]
  } else {
    allDeps = Object.keys(shrSpecs)
  }
  const specifiers: ResolvedDependencies = {}
  const shrDependencies: ResolvedDependencies = {}
  const shrOptionalDependencies: ResolvedDependencies = {}
  const shrDevDependencies: ResolvedDependencies = {}
  const nonOptional = new Set()
  const notProdOnly = new Set()

  Object.keys(shrSpecs).forEach((depName) => {
    if (allDeps.indexOf(depName) === -1) return
    specifiers[depName] = shrSpecs[depName]
    if (shr.dependencies && shr.dependencies[depName]) {
      shrDependencies[depName] = shr.dependencies[depName]
    } else if (shr.optionalDependencies && shr.optionalDependencies[depName]) {
      shrOptionalDependencies[depName] = shr.optionalDependencies[depName]
    } else if (shr.devDependencies && shr.devDependencies[depName]) {
      shrDevDependencies[depName] = shr.devDependencies[depName]
    }
  })
  if (shr.dependencies) {
    for (const dep of R.keys(shr.dependencies)) {
      if (!shrDependencies[dep] && shr.dependencies[dep].startsWith('link:')) {
        shrDependencies[dep] = shr.dependencies[dep]
      }
    }
  }

  const devDepRelativePaths = R.keys(shrDevDependencies)
    .map((pkgName: string) => refToRelative(shrDevDependencies[pkgName], pkgName))
    .filter((relPath) => relPath !== null) as string[]

  copyDependencySubTree(packages, devDepRelativePaths, shr, new Set(), warn, {registry: shr.registry, nonOptional, notProdOnly, dev: true})

  const depRelativePaths = R.keys(shrDependencies)
    .map((pkgName: string) => refToRelative(shrDependencies[pkgName], pkgName))
    .filter((relPath) => relPath !== null) as string[]

  copyDependencySubTree(packages, depRelativePaths, shr, new Set(), warn, {
    nonOptional,
    notProdOnly,
    registry: shr.registry,
  })

  if (shrOptionalDependencies) {
    const optionalDepRelativePaths = R.keys(shrOptionalDependencies)
      .map((pkgName: string) => refToRelative(shrOptionalDependencies[pkgName], pkgName))
      .filter((relPath) => relPath !== null) as string[]
    copyDependencySubTree(packages, optionalDepRelativePaths, shr, new Set(), warn, {registry: shr.registry, nonOptional, notProdOnly, optional: true})
  }

  copyDependencySubTree(packages, devDepRelativePaths, shr, new Set(), warn, {
    dev: true,
    nonOptional,
    notProdOnly,
    registry: shr.registry,
    walkOptionals: true,
  })

  copyDependencySubTree(packages, depRelativePaths, shr, new Set(), warn, {
    nonOptional,
    notProdOnly,
    registry: shr.registry,
    walkOptionals: true,
  })

  const result: Shrinkwrap = {
    registry: shr.registry,
    shrinkwrapVersion: SHRINKWRAP_VERSION,
    specifiers,
  }
  if (typeof shr.shrinkwrapMinorVersion === 'number') {
    result.shrinkwrapMinorVersion = shr.shrinkwrapMinorVersion
  }
  if (!R.isEmpty(packages)) {
    result.packages = packages
  }
  if (!R.isEmpty(shrDependencies)) {
    result.dependencies = shrDependencies
  }
  if (!R.isEmpty(shrOptionalDependencies)) {
    result.optionalDependencies = shrOptionalDependencies
  }
  if (!R.isEmpty(shrDevDependencies)) {
    result.devDependencies = shrDevDependencies
  }
  return result
}

function copyDependencySubTree (
  resolvedPackages: ResolvedPackages,
  depRelativePaths: string[],
  shr: Shrinkwrap,
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
    if (!shr.packages || !shr.packages[depRalativePath]) {
      // local dependencies don't need to be resolved in shrinkwrap.yaml
      // except local tarball dependencies
      if (depRalativePath.startsWith('link:') || depRalativePath.startsWith('file:') && !depRalativePath.endsWith('.tar.gz')) continue

      warn(`Cannot find resolution of ${depRalativePath} in shrinkwrap file`)
      continue
    }
    const depShr = shr.packages[depRalativePath]
    resolvedPackages[depRalativePath] = depShr
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
    copyDependencySubTree(resolvedPackages, newDependencies, shr, walked, warn, opts)
    if (!opts.walkOptionals) continue
    const newOptionalDependencies = R.keys(depShr.optionalDependencies)
      .map((pkgName: string) => refToRelative((depShr.optionalDependencies && depShr.optionalDependencies[pkgName]) as string, pkgName))
      .filter((relPath) => relPath !== null) as string[]
    copyDependencySubTree(resolvedPackages, newOptionalDependencies, shr, walked, warn, {...opts, optional: true})
  }
}
