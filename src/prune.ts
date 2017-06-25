import R = require('ramda')
import logger from 'pnpm-logger'
import {
  Shrinkwrap,
  Package,
  ResolvedPackages,
  ResolvedDependencies,
} from './types'

const SHRINKWRAP_VERSION = 3

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

  R.keys(shr.specifiers).forEach(depName => {
    if (allDeps.indexOf(depName) === -1) return
    specifiers[depName] = shr.specifiers[depName]
    if (shr.dependencies[depName]) {
      shrDependencies[depName] = shr.dependencies[depName]
    } else if (shr.optionalDependencies && shr.optionalDependencies[depName]) {
      shrOptionalDependencies[depName] = shr.optionalDependencies[depName]
    } else if (shr.devDependencies && shr.devDependencies[depName]) {
      shrDevDependencies[depName] = shr.devDependencies[depName]
    }
  })

  if (shrOptionalDependencies) {
    let optionalPkgIds: string[] = R.keys(shrOptionalDependencies)
      .map((pkgName: string) => getPkgShortId(shrOptionalDependencies[pkgName], pkgName))
    copyDependencySubTree(packages, optionalPkgIds, shr, [], {registry: shr.registry, nonOptional, optional: true})
  }

  if (shrDevDependencies) {
    let devPkgIds: string[] = R.keys(shrDevDependencies)
      .map((pkgName: string) => getPkgShortId(shrDevDependencies[pkgName], pkgName))
    copyDependencySubTree(packages, devPkgIds, shr, [], {registry: shr.registry, nonOptional, dev: true})
  }

  let pkgIds: string[] = R.keys(shrDependencies)
    .map((pkgName: string) => getPkgShortId(shrDependencies[pkgName], pkgName))

  copyDependencySubTree(packages, pkgIds, shr, [], {
    registry: shr.registry,
    nonOptional,
  })

  const result: Shrinkwrap = {
    shrinkwrapVersion: SHRINKWRAP_VERSION,
    specifiers,
    registry: shr.registry,
    dependencies: shrDependencies,
  }
  if (!R.isEmpty(packages)) {
    result.packages = packages
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
  pkgIds: string[],
  shr: Shrinkwrap,
  keypath: string[],
  opts: {
    registry: string,
    dev?: boolean,
    optional?: boolean,
    nonOptional: Set<string>,
  }
) {
  for (let pkgId of pkgIds) {
    if (keypath.indexOf(pkgId) !== -1) continue
    // local dependencies don't need to be resolved in shrinkwrap.yaml
    if (pkgId.indexOf('file:') === 0) continue
    if (!shr.packages || !shr.packages[pkgId]) {
      logger.warn(`Cannot find resolution of ${pkgId} in shrinkwrap file`)
      continue
    }
    const depShr = shr.packages[pkgId]
    resolvedPackages[pkgId] = depShr
    if (opts.optional && !opts.nonOptional.has(pkgId)) {
      depShr.optional = true
    } else {
      opts.nonOptional.add(pkgId)
      delete depShr.optional
    }
    if (opts.dev) {
      depShr.dev = true
    } else {
      delete depShr.dev
    }
    const newDependencies = R.keys(depShr.dependencies)
      .map((pkgName: string) => getPkgShortId(<string>(depShr.dependencies && depShr.dependencies[pkgName]), pkgName))
    const newKeypath = keypath.concat([pkgId])
    copyDependencySubTree(resolvedPackages, newDependencies, shr, newKeypath, opts)

    const newOptionalDependencies = R.keys(depShr.optionalDependencies)
      .map((pkgName: string) => getPkgShortId(<string>(depShr.optionalDependencies && depShr.optionalDependencies[pkgName]), pkgName))
    copyDependencySubTree(resolvedPackages, newOptionalDependencies, shr, newKeypath, Object.assign({}, opts, {optional: true}))
  }
}

export function getPkgShortId (
  reference: string,
  pkgName: string
) {
  if (reference.indexOf('/') === -1) {
    return `/${pkgName}/${reference}`
  }
  return reference
}
