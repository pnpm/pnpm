import {Resolution} from '@pnpm/resolver-base'
import {Dependencies, PackageJson} from '@pnpm/types'
import { readPackageFromDir } from '@pnpm/utils'
import * as dp from 'dependency-path'
import getNpmTarballUrl from 'get-npm-tarball-url'
import {
  DependencyShrinkwrap,
  PackageSnapshot,
  prune as pruneShrinkwrap,
  ResolvedDependencies,
  Shrinkwrap,
  ShrinkwrapResolution,
} from 'pnpm-shrinkwrap'
import R = require('ramda')
import {absolutePathToRef} from '../fs/shrinkwrap'
import {DepGraphNode, DepGraphNodesByDepPath} from './resolvePeers'

export default function (
  depGraph: DepGraphNodesByDepPath,
  shrinkwrap: Shrinkwrap,
  pkg: PackageJson,
): {
  newShrinkwrap: Shrinkwrap,
  pendingRequiresBuilds: PendingRequiresBuild[],
} {
  shrinkwrap.packages = shrinkwrap.packages || {}
  const pendingRequiresBuilds = [] as PendingRequiresBuild[]
  for (const depPath of R.keys(depGraph)) {
    const relDepPath = dp.relative(shrinkwrap.registry, depPath)
    const result = R.partition(
      (child) => depGraph[depPath].optionalDependencies.has(depGraph[child.depPath].name),
      R.keys(depGraph[depPath].children).map((alias) => ({alias, depPath: depGraph[depPath].children[alias]})),
    )
    shrinkwrap.packages[relDepPath] = toShrDependency(pendingRequiresBuilds, depGraph[depPath].additionalInfo, {
      depGraph,
      depPath,
      prevSnapshot: shrinkwrap.packages[relDepPath],
      registry: shrinkwrap.registry,
      relDepPath,
      updatedDeps: result[1],
      updatedOptionalDeps: result[0],
    })
  }
  return {
    newShrinkwrap: pruneShrinkwrap(shrinkwrap, pkg),
    pendingRequiresBuilds,
  }
}

export interface PendingRequiresBuild {
  relativeDepPath: string,
  absoluteDepPath: string,
  value: Promise<boolean>,
}

function toShrDependency (
  pendingRequiresBuilds: PendingRequiresBuild[],
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
    relDepPath: string,
    registry: string,
    updatedDeps: Array<{alias: string, depPath: string}>,
    updatedOptionalDeps: Array<{alias: string, depPath: string}>,
    depGraph: DepGraphNodesByDepPath,
    prevSnapshot?: PackageSnapshot,
  },
): DependencyShrinkwrap {
  const depNode = opts.depGraph[opts.depPath]
  const shrResolution = toShrResolution(
    {name: depNode.name, version: depNode.version},
    opts.relDepPath,
    depNode.resolution,
    opts.registry,
  )
  const newResolvedDeps = updateResolvedDeps(
    opts.prevSnapshot && opts.prevSnapshot.dependencies || {},
    opts.updatedDeps,
    opts.registry,
    opts.depGraph,
  )
  const newResolvedOptionalDeps = updateResolvedDeps(
    opts.prevSnapshot && opts.prevSnapshot.optionalDependencies || {},
    opts.updatedOptionalDeps,
    opts.registry,
    opts.depGraph,
  )
  const result = {
    resolution: shrResolution,
  }
  // tslint:disable:no-string-literal
  if (dp.isAbsolute(opts.relDepPath)) {
    result['name'] = depNode.name

    // There is no guarantee that a non-npmjs.org-hosted package
    // is going to have a version field
    if (depNode.version) {
      result['version'] = depNode.version
    }
  }
  if (!R.isEmpty(newResolvedDeps)) {
    result['dependencies'] = newResolvedDeps
  }
  if (!R.isEmpty(newResolvedOptionalDeps)) {
    result['optionalDependencies'] = newResolvedOptionalDeps
  }
  if (depNode.dev && !depNode.prod) {
    result['dev'] = true
  } else if (depNode.prod && !depNode.dev) {
    result['dev'] = false
  }
  if (depNode.optional) {
    result['optional'] = true
  }
  if (opts.depPath !== depNode.id) {
    result['id'] = depNode.id
  }
  if (pkg.peerDependencies) {
    result['peerDependencies'] = pkg.peerDependencies
  }
  if (pkg.engines) {
    for (const engine of R.keys(pkg.engines)) {
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
  if (opts.prevSnapshot) {
    if (opts.prevSnapshot.requiresBuild) {
      result['requiresBuild'] = opts.prevSnapshot.requiresBuild
    }
    if (opts.prevSnapshot.prepare) {
      result['prepare'] = opts.prevSnapshot.prepare
    }
  } else if (depNode.prepare) {
    result['prepare'] = true
    result['requiresBuild'] = true
  } else {
    pendingRequiresBuilds.push({
      absoluteDepPath: opts.depPath,
      relativeDepPath: opts.relDepPath,
      value: (async () => {
        // The npm team suggests to always read the package.json for deciding whether the package has lifecycle scripts
        const filesResponse = await depNode.fetchingFiles
        const pkgJson = await readPackageFromDir(depNode.centralLocation)
        return Boolean(
          pkgJson.scripts && (pkgJson.scripts.preinstall || pkgJson.scripts.install || pkgJson.scripts.postinstall) ||
          filesResponse.filenames.indexOf('binding.gyp') !== -1 ||
            filesResponse.filenames.some((filename) => !!filename.match(/^[.]hooks[\\/]/)), // TODO: optimize this
        )
      })(),
    })
  }
  depNode.requiresBuild = result['requiresBuild']
  // tslint:enable:no-string-literal
  return result
}

// previous resolutions should not be removed from shrinkwrap
// as installation might not reanalize the whole dependency graph
// the `depth` property defines how deep should dependencies be checked
function updateResolvedDeps (
  prevResolvedDeps: ResolvedDependencies,
  updatedDeps: Array<{alias: string, depPath: string}>,
  registry: string,
  depGraph: DepGraphNodesByDepPath,
) {
  const newResolvedDeps = R.fromPairs<string>(
    updatedDeps
      .map((dep): R.KeyValuePair<string, string> => {
        const depNode = depGraph[dep.depPath]
        return [
          dep.alias,
          absolutePathToRef(depNode.absolutePath, {
            alias: dep.alias,
            realName: depNode.name,
            resolution: depNode.resolution,
            standardRegistry: registry,
          }),
        ]
      }),
  )
  return R.merge(
    prevResolvedDeps,
    newResolvedDeps,
  )
}

function toShrResolution (
  pkg: {
    name: string,
    version: string,
  },
  relDepPath: string,
  resolution: Resolution,
  registry: string,
): ShrinkwrapResolution {
  // tslint:disable:no-string-literal
  if (dp.isAbsolute(relDepPath) || resolution.type !== undefined || !resolution['integrity']) {
    return resolution as ShrinkwrapResolution
  }
  const base = registry !== resolution['registry'] ? {registry: resolution['registry']} : {}
  // Sometimes packages are hosted under non-standard tarball URLs.
  // For instance, when they are hosted on npm Enterprise. See https://github.com/pnpm/pnpm/issues/867
  // Or in othere weird cases, like https://github.com/pnpm/pnpm/issues/1072
  if (getNpmTarballUrl(pkg.name, pkg.version, {registry}) !== resolution['tarball']) {
    return {
      ...base,
      integrity: resolution['integrity'],
      tarball: relativeTarball(resolution['tarball'], registry),
    }
  }
  return {
    ...base,
    integrity: resolution['integrity'],
  }
  // tslint:enable:no-string-literal
}

function relativeTarball (tarball: string, registry: string) {
  if (tarball.substr(0, registry.length) === registry) {
    return tarball.substr(registry.length - 1)
  }
  return tarball
}
