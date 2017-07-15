import * as dp from 'dependency-path'
import {pkgIdToRef} from '../fs/shrinkwrap'
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
  for (const resolvedId of R.keys(pkgsToLink)) {
    const shortId = dp.relative(shrinkwrap.registry, resolvedId)
    const result = R.partition((childResolvedId: string) => pkgsToLink[resolvedId].optionalDependencies.has(pkgsToLink[childResolvedId].name), pkgsToLink[resolvedId].children)
    shrinkwrap.packages[shortId] = toShrDependency({
      resolvedId,
      id: pkgsToLink[resolvedId].id,
      shortId,
      resolution: pkgsToLink[resolvedId].resolution,
      updatedOptionalDeps: result[0],
      updatedDeps: result[1],
      registry: shrinkwrap.registry,
      pkgsToLink,
      prevResolvedDeps: shrinkwrap.packages[shortId] && shrinkwrap.packages[shortId].dependencies || {},
      prevResolvedOptionalDeps: shrinkwrap.packages[shortId] && shrinkwrap.packages[shortId].optionalDependencies || {},
      dev: pkgsToLink[resolvedId].dev,
      optional: pkgsToLink[resolvedId].optional,
    })
  }
  return pruneShrinkwrap(shrinkwrap, pkg)
}

function toShrDependency (
  opts: {
    resolvedId: string,
    id: string,
    shortId: string,
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
  const shrResolution = toShrResolution(opts.shortId, opts.resolution)
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
  if (opts.resolvedId !== opts.id) {
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
        pkgIdToRef(dep.resolvedId, dep.name, dep.resolution, registry)
      ])
  )
  return R.merge(
    prevResolvedDeps,
    newResolvedDeps
  )
}

function toShrResolution (shortId: string, resolution: Resolution): ShrinkwrapResolution {
  if (shortId.startsWith('/') && resolution.type === undefined && resolution.integrity) {
    return {
      integrity: resolution.integrity,
    }
  }
  return resolution
}
