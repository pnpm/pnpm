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
import {Package} from '../types'

export default function (
  pkgsToLink: DependencyTreeNodeMap,
  shrinkwrap: Shrinkwrap,
  pkg: Package
): Shrinkwrap {
  shrinkwrap.packages = shrinkwrap.packages || {}
  for (const dependencyAbsolutePath of R.keys(pkgsToLink)) {
    const dependencyPath = dp.relative(shrinkwrap.registry, dependencyAbsolutePath)
    const result = R.partition(
      (childResolvedId: string) => pkgsToLink[dependencyAbsolutePath].optionalDependencies.has(pkgsToLink[childResolvedId].name),
      pkgsToLink[dependencyAbsolutePath].children
    )
    shrinkwrap.packages[dependencyPath] = toShrDependency({
      dependencyAbsolutePath,
      id: pkgsToLink[dependencyAbsolutePath].id,
      dependencyPath,
      resolution: pkgsToLink[dependencyAbsolutePath].resolution,
      updatedOptionalDeps: result[0],
      updatedDeps: result[1],
      registry: shrinkwrap.registry,
      pkgsToLink,
      prevResolvedDeps: shrinkwrap.packages[dependencyPath] && shrinkwrap.packages[dependencyPath].dependencies || {},
      prevResolvedOptionalDeps: shrinkwrap.packages[dependencyPath] && shrinkwrap.packages[dependencyPath].optionalDependencies || {},
      dev: pkgsToLink[dependencyAbsolutePath].dev,
      optional: pkgsToLink[dependencyAbsolutePath].optional,
    })
  }
  return pruneShrinkwrap(shrinkwrap, pkg)
}

function toShrDependency (
  opts: {
    dependencyAbsolutePath: string,
    id: string,
    dependencyPath: string,
    resolution: Resolution,
    registry: string,
    updatedDeps: string[],
    updatedOptionalDeps: string[],
    pkgsToLink: DependencyTreeNodeMap,
    prevResolvedDeps: ResolvedDependencies,
    prevResolvedOptionalDeps: ResolvedDependencies,
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
  if (!R.isEmpty(newResolvedDeps)) {
    result['dependencies'] = newResolvedDeps
  }
  if (!R.isEmpty(newResolvedOptionalDeps)) {
    result['optionalDependencies'] = newResolvedOptionalDeps
  }
  if (opts.dev) {
    result['dev'] = true
  }
  if (opts.optional) {
    result['optional'] = true
  }
  if (opts.dependencyAbsolutePath !== opts.id) {
    result['id'] = opts.id
  }
  return result
}

// previous resolutions should not be removed from shrinkwrap
// as installation might not reanalize the whole dependency tree
// the `depth` property defines how deep should dependencies be checked
function updateResolvedDeps (
  prevResolvedDeps: ResolvedDependencies,
  updatedDeps: string[],
  registry: string,
  pkgsToLink: DependencyTreeNodeMap
) {
  const newResolvedDeps = R.fromPairs<string>(
    R.props<DependencyTreeNode>(updatedDeps, pkgsToLink)
      .map((dep): R.KeyValuePair<string, string> => [
        dep.name,
        absolutePathToRef(dep.absolutePath, dep.name, dep.resolution, registry)
      ])
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
