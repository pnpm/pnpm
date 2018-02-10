import * as dp from 'dependency-path'
import {absolutePathToRef} from '../fs/shrinkwrap'
import {
  Shrinkwrap,
  DependencyShrinkwrap,
  ShrinkwrapResolution,
  ResolvedDependencies,
  prune as pruneShrinkwrap,
} from 'pnpm-shrinkwrap'
import {DependencyTreeNodeMap, DependencyTreeNode} from './resolvePeers'
import {Resolution} from '@pnpm/resolver-base'
import R = require('ramda')
import {PackageJson, Dependencies} from '@pnpm/types'

export default function (
  pkgsToLink: DependencyTreeNodeMap,
  shrinkwrap: Shrinkwrap,
  pkg: PackageJson
): Shrinkwrap {
  shrinkwrap.packages = shrinkwrap.packages || {}
  for (const depPath of R.keys(pkgsToLink)) {
    const relDepPath = dp.relative(shrinkwrap.registry, depPath)
    const result = R.partition(
      (child) => pkgsToLink[depPath].optionalDependencies.has(pkgsToLink[child.nodeId].name),
      R.keys(pkgsToLink[depPath].children).map(alias => ({alias, nodeId: pkgsToLink[depPath].children[alias]}))
    )
    shrinkwrap.packages[relDepPath] = toShrDependency(pkgsToLink[depPath].additionalInfo, {
      depPath,
      name: pkgsToLink[depPath].name,
      version: pkgsToLink[depPath].version,
      id: pkgsToLink[depPath].id,
      relDepPath,
      resolution: pkgsToLink[depPath].resolution,
      updatedOptionalDeps: result[0],
      updatedDeps: result[1],
      registry: shrinkwrap.registry,
      pkgsToLink,
      prevResolvedDeps: shrinkwrap.packages[relDepPath] && shrinkwrap.packages[relDepPath].dependencies || {},
      prevResolvedOptionalDeps: shrinkwrap.packages[relDepPath] && shrinkwrap.packages[relDepPath].optionalDependencies || {},
      prod: pkgsToLink[depPath].prod,
      dev: pkgsToLink[depPath].dev,
      optional: pkgsToLink[depPath].optional,
    })
  }
  return pruneShrinkwrap(shrinkwrap, pkg)
}

function toShrDependency (
  pkg: {
    deprecated?: string,
    peerDependencies?: Dependencies,
    bundleDependencies?: string[],
    bundledDependencies?: string[],
    engines?: {
      node?: string,
      npm?: string,
    },
    cpu?: string[],
    os?: string[],
  },
  opts: {
    depPath: string,
    name: string,
    version: string,
    id: string,
    relDepPath: string,
    resolution: Resolution,
    registry: string,
    updatedDeps: {alias: string, nodeId: string}[],
    updatedOptionalDeps: {alias: string, nodeId: string}[],
    pkgsToLink: DependencyTreeNodeMap,
    prevResolvedDeps: ResolvedDependencies,
    prevResolvedOptionalDeps: ResolvedDependencies,
    prod: boolean,
    dev: boolean,
    optional: boolean,
  }
): DependencyShrinkwrap {
  const shrResolution = toShrResolution(opts.relDepPath, opts.resolution, opts.registry)
  const newResolvedDeps = updateResolvedDeps(opts.prevResolvedDeps, opts.updatedDeps, opts.registry, opts.pkgsToLink)
  const newResolvedOptionalDeps = updateResolvedDeps(opts.prevResolvedOptionalDeps, opts.updatedOptionalDeps, opts.registry, opts.pkgsToLink)
  const result = {
    resolution: shrResolution
  }
  if (dp.isAbsolute(opts.relDepPath)) {
    result['name'] = opts.name

    // There is no guarantee that a non-npmjs.org-hosted package
    // is going to have a version field
    if (opts.version) {
      result['version'] = opts.version
    }
  }
  if (!R.isEmpty(newResolvedDeps)) {
    result['dependencies'] = newResolvedDeps
  }
  if (!R.isEmpty(newResolvedOptionalDeps)) {
    result['optionalDependencies'] = newResolvedOptionalDeps
  }
  if (opts.dev && !opts.prod) {
    result['dev'] = true
  } else if (opts.prod && !opts.dev) {
    result['dev'] = false
  }
  if (opts.optional) {
    result['optional'] = true
  }
  if (opts.depPath !== opts.id) {
    result['id'] = opts.id
  }
  if (pkg.peerDependencies) {
    result['peerDependencies'] = pkg.peerDependencies
  }
  if (pkg.engines) {
    for (let engine of R.keys(pkg.engines)) {
      if (pkg.engines[engine] === '*') continue
      result['engines'] = result['engines'] || {}
      result['engines'][engine] = pkg.engines[engine]
    }
  }
  if (pkg.cpu) {
    result['cpu'] = pkg.cpu
  }
  if (pkg.os) {
    result['os'] = pkg.os
  }
  if (pkg.bundledDependencies || pkg.bundleDependencies) {
    result['bundledDependencies'] = pkg.bundledDependencies || pkg.bundleDependencies
  }
  if (pkg.deprecated) {
    result['deprecated'] = pkg.deprecated
  }
  return result
}

// previous resolutions should not be removed from shrinkwrap
// as installation might not reanalize the whole dependency tree
// the `depth` property defines how deep should dependencies be checked
function updateResolvedDeps (
  prevResolvedDeps: ResolvedDependencies,
  updatedDeps: {alias: string, nodeId: string}[],
  registry: string,
  pkgsToLink: DependencyTreeNodeMap
) {
  const newResolvedDeps = R.fromPairs<string>(
    updatedDeps
      .map((dep): R.KeyValuePair<string, string> => {
        const pkgToLink = pkgsToLink[dep.nodeId]
        return [
          dep.alias,
          absolutePathToRef(pkgToLink.absolutePath, {
            alias: dep.alias,
            realName: pkgToLink.name,
            resolution: pkgToLink.resolution,
            standardRegistry: registry,
          })
        ]
      })
  )
  return R.merge(
    prevResolvedDeps,
    newResolvedDeps
  )
}

function toShrResolution (
  relDepPath: string,
  resolution: Resolution,
  registry: string
): ShrinkwrapResolution {
  if (dp.isAbsolute(relDepPath) || resolution.type !== undefined || !resolution['integrity']) {
    return resolution as ShrinkwrapResolution
  }
  // This might be not the best solution to identify non-standard tarball URLs in the long run
  // but it at least solves the issues with npm Enterprise. See https://github.com/pnpm/pnpm/issues/867
  if (!resolution['tarball'].includes('/-/')) {
    return {
      integrity: resolution['integrity'],
      tarball: relativeTarball(resolution['tarball'], registry),
    }
  }
  return {
    integrity: resolution['integrity'],
  }
}

function relativeTarball (tarball: string, registry: string) {
  if (tarball.substr(0, registry.length) === registry) {
    return tarball.substr(registry.length - 1)
  }
  return tarball
}
