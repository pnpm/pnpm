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

  copyShrinkwrap({
    devRelPaths: resolvedDepsToRelDepPaths(shrDevDependencies),
    oldResolutions: shr.packages || {},
    optionalRelPaths: resolvedDepsToRelDepPaths(shrOptionalDependencies),
    packages,
    prodRelPaths: resolvedDepsToRelDepPaths(shrDependencies),
    registry: shr.registry,
    warn,
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

function copyShrinkwrap (
  opts: {
    devRelPaths: string[],
    oldResolutions: ResolvedPackages,
    optionalRelPaths: string[],
    packages: ResolvedPackages,
    prodRelPaths: string[],
    registry: string,
    warn: (msg: string) => void,
  },
) {
  const nonOptional = new Set()
  const notProdOnly = new Set()

  copyDependencySubTree(opts.packages, opts.devRelPaths, opts.oldResolutions, new Set(), opts.warn, {
    dev: true,
    nonOptional,
    notProdOnly,
    registry: opts.registry,
  })

  copyDependencySubTree(opts.packages, opts.prodRelPaths, opts.oldResolutions, new Set(), opts.warn, {
    nonOptional,
    notProdOnly,
    registry: opts.registry,
  })

  copyDependencySubTree(opts.packages, opts.optionalRelPaths, opts.oldResolutions, new Set(), opts.warn, {
    nonOptional,
    notProdOnly,
    optional: true,
    registry: opts.registry,
  })

  copyDependencySubTree(opts.packages, opts.devRelPaths, opts.oldResolutions, new Set(), opts.warn, {
    dev: true,
    nonOptional,
    notProdOnly,
    registry: opts.registry,
    walkOptionals: true,
  })

  copyDependencySubTree(opts.packages, opts.prodRelPaths, opts.oldResolutions, new Set(), opts.warn, {
    nonOptional,
    notProdOnly,
    registry: opts.registry,
    walkOptionals: true,
  })
}

function resolvedDepsToRelDepPaths (deps: ResolvedDependencies) {
  return R.keys(deps)
    .map((pkgName: string) => refToRelative(deps[pkgName], pkgName))
    .filter((relPath) => relPath !== null) as string[]
}

function copyDependencySubTree (
  newResolutions: ResolvedPackages,
  depRelativePaths: string[],
  oldResolutions: ResolvedPackages,
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
    if (!oldResolutions[depRalativePath]) {
      // local dependencies don't need to be resolved in shrinkwrap.yaml
      // except local tarball dependencies
      if (depRalativePath.startsWith('link:') || depRalativePath.startsWith('file:') && !depRalativePath.endsWith('.tar.gz')) continue

      warn(`Cannot find resolution of ${depRalativePath} in shrinkwrap file`)
      continue
    }
    const depShr = oldResolutions[depRalativePath]
    newResolutions[depRalativePath] = depShr
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
    copyDependencySubTree(newResolutions, newDependencies, oldResolutions, walked, warn, opts)
    if (!opts.walkOptionals) continue
    const newOptionalDependencies = R.keys(depShr.optionalDependencies)
      .map((pkgName: string) => refToRelative((depShr.optionalDependencies && depShr.optionalDependencies[pkgName]) as string, pkgName))
      .filter((relPath) => relPath !== null) as string[]
    copyDependencySubTree(newResolutions, newOptionalDependencies, oldResolutions, walked, warn, {...opts, optional: true})
  }
}
