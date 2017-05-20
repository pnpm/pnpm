import {
  Shrinkwrap,
  DependencyShrinkwrap,
  pkgShortId,
  pkgIdToRef,
  ResolvedDependencies,
} from '../fs/shrinkwrap'
import {DependencyTreeNodeMap, DependencyTreeNode} from './resolvePeers'
import {Resolution} from '../resolve'
import R = require('ramda')

export default function (pkgsToLink: DependencyTreeNodeMap, shrinkwrap: Shrinkwrap) {
  for (const resolvedId of R.keys(pkgsToLink)) {
    const shortId = pkgShortId(resolvedId, shrinkwrap.registry)
    shrinkwrap.packages[shortId] = toShrDependency({
      shortId,
      resolution: pkgsToLink[resolvedId].resolution,
      updatedDeps: pkgsToLink[resolvedId].children,
      registry: shrinkwrap.registry,
      pkgsToLink,
      prevResolvedDeps: shrinkwrap.packages[shortId] && shrinkwrap.packages[shortId]['dependencies'] || {},
    })
  }
}

function toShrDependency (
  opts: {
    shortId: string,
    resolution: Resolution,
    registry: string,
    updatedDeps: string[],
    pkgsToLink: DependencyTreeNodeMap,
    prevResolvedDeps: ResolvedDependencies,
  }
): DependencyShrinkwrap {
  const shrResolution = toShrResolution(opts.shortId, opts.resolution)
  const newResolvedDeps = updateResolvedDeps(opts.prevResolvedDeps, opts.updatedDeps, opts.registry, opts.pkgsToLink)
  if (!R.isEmpty(newResolvedDeps)) {
    return {
      resolution: shrResolution,
      dependencies: newResolvedDeps,
    }
  }
  if (typeof shrResolution === 'string') return shrResolution
  return {
    resolution: shrResolution
  }
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

function toShrResolution (shortId: string, resolution: Resolution): string | Resolution {
  if (shortId.startsWith('/') && resolution.type === undefined && resolution.shasum) {
    return resolution.shasum
  }
  return resolution
}
