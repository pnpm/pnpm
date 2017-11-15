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
import {Resolution} from 'package-store'
import R = require('ramda')
import {PackageJson} from '@pnpm/types'
import {PackageManifest} from '../types'

export default function (
  pkgsToLink: DependencyTreeNodeMap,
  shrinkwrap: Shrinkwrap,
  pkg: PackageJson
): Shrinkwrap {
  shrinkwrap.packages = shrinkwrap.packages || {}
  for (const dependencyAbsolutePath of R.keys(pkgsToLink)) {
    const dependencyPath = dp.relative(shrinkwrap.registry, dependencyAbsolutePath)
    const result = R.partition(
      (child) => pkgsToLink[dependencyAbsolutePath].optionalDependencies.has(pkgsToLink[child.nodeId].name),
      R.keys(pkgsToLink[dependencyAbsolutePath].children).map(alias => ({alias, nodeId: pkgsToLink[dependencyAbsolutePath].children[alias]}))
    )
    shrinkwrap.packages[dependencyPath] = toShrDependency(pkgsToLink[dependencyAbsolutePath].pkg, {
      dependencyAbsolutePath,
      name: pkgsToLink[dependencyAbsolutePath].name,
      version: pkgsToLink[dependencyAbsolutePath].version,
      id: pkgsToLink[dependencyAbsolutePath].id,
      dependencyPath,
      resolution: pkgsToLink[dependencyAbsolutePath].resolution,
      updatedOptionalDeps: result[0],
      updatedDeps: result[1],
      registry: shrinkwrap.registry,
      pkgsToLink,
      prevResolvedDeps: shrinkwrap.packages[dependencyPath] && shrinkwrap.packages[dependencyPath].dependencies || {},
      prevResolvedOptionalDeps: shrinkwrap.packages[dependencyPath] && shrinkwrap.packages[dependencyPath].optionalDependencies || {},
      prod: pkgsToLink[dependencyAbsolutePath].prod,
      dev: pkgsToLink[dependencyAbsolutePath].dev,
      optional: pkgsToLink[dependencyAbsolutePath].optional,
    })
  }
  return pruneShrinkwrap(shrinkwrap, pkg)
}

function toShrDependency (
  pkg: PackageManifest,
  opts: {
    dependencyAbsolutePath: string,
    name: string,
    version: string,
    id: string,
    dependencyPath: string,
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
  const shrResolution = toShrResolution(opts.dependencyPath, opts.resolution, opts.registry)
  const newResolvedDeps = updateResolvedDeps(opts.prevResolvedDeps, opts.updatedDeps, opts.registry, opts.pkgsToLink)
  const newResolvedOptionalDeps = updateResolvedDeps(opts.prevResolvedOptionalDeps, opts.updatedOptionalDeps, opts.registry, opts.pkgsToLink)
  const result = {
    resolution: shrResolution
  }
  if (dp.isAbsolute(opts.dependencyPath)) {
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
  if (opts.dependencyAbsolutePath !== opts.id) {
    result['id'] = opts.id
  }
  if (pkg.peerDependencies) {
    const ownDeps = new Set(
      R.keys(pkg.dependencies).concat(R.keys(pkg.optionalDependencies))
    )
    for (let peer of R.keys(pkg.peerDependencies)) {
      if (ownDeps.has(peer)) continue
      result['peerDependencies'] = result['peerDependencies'] || {}
      result['peerDependencies'][peer] = pkg.peerDependencies[peer]
    }
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
          absolutePathToRef(pkgToLink.absolutePath, pkgToLink.name, pkgToLink.resolution, registry)
        ]
      })
  )
  return R.merge(
    prevResolvedDeps,
    newResolvedDeps
  )
}

function toShrResolution (
  dependencyPath: string,
  resolution: Resolution,
  registry: string
): ShrinkwrapResolution {
  if (dp.isAbsolute(dependencyPath) || resolution.type !== undefined || !resolution.integrity) {
    return resolution
  }
  // This might be not the best solution to identify non-standard tarball URLs in the long run
  // but it at least solves the issues with npm Enterprise. See https://github.com/pnpm/pnpm/issues/867
  if (!resolution.tarball.includes('/-/')) {
    return {
      integrity: resolution.integrity,
      tarball: relativeTarball(resolution.tarball, registry),
    }
  }
  return {
    integrity: resolution.integrity,
  }
}

function relativeTarball (tarball: string, registry: string) {
  if (tarball.substr(0, registry.length) === registry) {
    return tarball.substr(registry.length - 1)
  }
  return tarball
}
