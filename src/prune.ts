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

  if (shr.optionalDependencies) {
    let optionalPkgIds: string[] = R.keys(shr.optionalDependencies)
      .map((pkgName: string) => getPkgShortId(shr!.optionalDependencies![pkgName], pkgName))
    copyDependencySubTree(packages, optionalPkgIds, shr, [], {registry: shr.registry, optional: true})
  }

  if (shr.devDependencies) {
    let devPkgIds: string[] = R.keys(shr.devDependencies)
      .map((pkgName: string) => getPkgShortId(shr!.devDependencies![pkgName], pkgName))
    copyDependencySubTree(packages, devPkgIds, shr, [], {registry: shr.registry, dev: true})
  }

  let pkgIds: string[] = dependencies
    .map((pkgName: string) => getPkgShortId(shr.dependencies[pkgName], pkgName))

  copyDependencySubTree(packages, pkgIds, shr, [], {
    registry: shr.registry,
  })

  const allDeps = R.reduce(R.union, [], [optionalDependencies, devDependencies, dependencies])
  const specifiers: ResolvedDependencies = {}
  const shrDependencies: ResolvedDependencies = {}
  const shrOptionalDependencies: ResolvedDependencies = {}
  const shrDevDependencies: ResolvedDependencies = {}

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

  const result = {
    version: SHRINKWRAP_VERSION,
    specifiers,
    registry: shr.registry,
    dependencies: shrDependencies,
    optionalDependencies: shrOptionalDependencies,
    devDependencies: shrDevDependencies,
    packages,
  }
  if (R.isEmpty(result.optionalDependencies)) {
    delete result.optionalDependencies
  }
  if (R.isEmpty(result.devDependencies)) {
    delete result.devDependencies
  }
  return result
}

function copyDependencyTree (
  resolvedPackages: ResolvedPackages,
  shr: Shrinkwrap,
  opts: {
    registry: string,
    dependencies: string[],
    dev?: boolean,
    optional?: boolean,
  }
) {
  let pkgIds: string[] = opts.dependencies
    .map((pkgName: string) => getPkgShortId(shr.dependencies[pkgName], pkgName))

  copyDependencySubTree(resolvedPackages, pkgIds, shr, [], opts)

  if (shr.optionalDependencies) {
    let optionalPkgIds: string[] = R.keys(shr.optionalDependencies)
      .map((pkgName: string) => getPkgShortId(shr!.optionalDependencies![pkgName], pkgName))
    copyDependencySubTree(resolvedPackages, optionalPkgIds, shr, [], Object.assign({}, opts, {optional: true}))
  }
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
  }
) {
  for (let pkgId of pkgIds) {
    if (keypath.indexOf(pkgId) !== -1) continue
    if (!shr.packages[pkgId]) {
      logger.warn(`Cannot find resolution of ${pkgId} in shrinkwrap file`)
      continue
    }
    const depShr = shr.packages[pkgId]
    resolvedPackages[pkgId] = depShr
    if (opts.optional) {
      depShr.optional = true
    } else {
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
