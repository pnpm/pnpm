import logger from '@pnpm/logger'
import {refToRelative} from 'dependency-path'
import R = require('ramda')
import {SHRINKWRAP_VERSION} from './constants'
import {
  Package,
  ResolvedDependencies,
  ResolvedPackages,
  Shrinkwrap,
} from './types'

export default function prune (shr: Shrinkwrap, pkg: Package): Shrinkwrap {
  const packages: ResolvedPackages = {}
  const optionalDependencies = R.keys(pkg.optionalDependencies)
  const dependencies = R.difference(R.keys(pkg.dependencies), optionalDependencies)
  const devDependencies = R.difference(R.difference(R.keys(pkg.devDependencies), optionalDependencies), dependencies)

  const allDeps = R.reduce(R.union, [], [optionalDependencies, devDependencies, dependencies])
  const specifiers: ResolvedDependencies = {}
  const shrDependencies: ResolvedDependencies = {}
  const shrOptionalDependencies: ResolvedDependencies = {}
  const shrDevDependencies: ResolvedDependencies = {}
  const nonOptional = new Set()

  R.keys(shr.specifiers).forEach((depName) => {
    if (allDeps.indexOf(depName) === -1) return
    specifiers[depName] = shr.specifiers[depName]
    if (shr.dependencies && shr.dependencies[depName]) {
      shrDependencies[depName] = shr.dependencies[depName]
    } else if (shr.optionalDependencies && shr.optionalDependencies[depName]) {
      shrOptionalDependencies[depName] = shr.optionalDependencies[depName]
    } else if (shr.devDependencies && shr.devDependencies[depName]) {
      shrDevDependencies[depName] = shr.devDependencies[depName]
    }
  })

  const devDepRelativePaths: string[] = R.keys(shrDevDependencies)
    .map((pkgName: string) => refToRelative(shrDevDependencies[pkgName], pkgName))

  copyDependencySubTree(packages, devDepRelativePaths, shr, new Set(), {registry: shr.registry, nonOptional, dev: true})

  const depRelativePaths: string[] = R.keys(shrDependencies)
    .map((pkgName: string) => refToRelative(shrDependencies[pkgName], pkgName))

  copyDependencySubTree(packages, depRelativePaths, shr, new Set(), {
    nonOptional,
    registry: shr.registry,
  })

  if (shrOptionalDependencies) {
    const optionalDepRelativePaths: string[] = R.keys(shrOptionalDependencies)
      .map((pkgName: string) => refToRelative(shrOptionalDependencies[pkgName], pkgName))
    copyDependencySubTree(packages, optionalDepRelativePaths, shr, new Set(), {registry: shr.registry, nonOptional, optional: true})
  }

  copyDependencySubTree(packages, devDepRelativePaths, shr, new Set(), {registry: shr.registry, nonOptional, dev: true, walkOptionals: true})

  copyDependencySubTree(packages, depRelativePaths, shr, new Set(), {
    nonOptional,
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
  opts: {
    registry: string,
    dev?: boolean,
    optional?: boolean,
    nonOptional: Set<string>,
    walkOptionals?: boolean,
  },
) {
  for (const depRalativePath of depRelativePaths) {
    if (walked.has(depRalativePath)) continue
    walked.add(depRalativePath)
    if (!shr.packages || !shr.packages[depRalativePath]) {
      // local dependencies don't need to be resolved in shrinkwrap.yaml
      // except local tarball dependencies
      if (depRalativePath.startsWith('file:') && !depRalativePath.endsWith('.tar.gz')) continue

      logger.warn(`Cannot find resolution of ${depRalativePath} in shrinkwrap file`)
      continue
    }
    const depShr = shr.packages[depRalativePath]
    const checkDev = !opts.walkOptionals || !resolvedPackages[depRalativePath]
    resolvedPackages[depRalativePath] = depShr
    if (opts.optional && !opts.nonOptional.has(depRalativePath)) {
      depShr.optional = true
    } else {
      opts.nonOptional.add(depRalativePath)
      delete depShr.optional
    }
    if (checkDev) {
      if (opts.dev) {
        depShr.dev = true
      } else if (depShr.dev === true) { // keeping if dev is explicitly false
        delete depShr.dev
      } else if (depShr.dev === undefined) {
        depShr.dev = false
      }
    }
    const newDependencies = R.keys(depShr.dependencies)
      .map((pkgName: string) => refToRelative((depShr.dependencies && depShr.dependencies[pkgName]) as string, pkgName))
    copyDependencySubTree(resolvedPackages, newDependencies, shr, walked, opts)
    if (!opts.walkOptionals) continue
    const newOptionalDependencies = R.keys(depShr.optionalDependencies)
      .map((pkgName: string) => refToRelative((depShr.optionalDependencies && depShr.optionalDependencies[pkgName]) as string, pkgName))
    copyDependencySubTree(resolvedPackages, newOptionalDependencies, shr, walked, {...opts, optional: true})
  }
}
