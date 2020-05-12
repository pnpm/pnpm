import logger from '@pnpm/logger'
import {
  Lockfile,
  LockfileResolution,
  PackageSnapshot,
  pruneSharedLockfile,
  ResolvedDependencies,
} from '@pnpm/prune-lockfile'
import { Resolution } from '@pnpm/resolver-base'
import {
  Dependencies,
  PeerDependenciesMeta,
  Registries,
} from '@pnpm/types'
import * as dp from 'dependency-path'
import getNpmTarballUrl from 'get-npm-tarball-url'
import R = require('ramda')
import { depPathToRef } from './lockfile'
import { DependenciesGraph } from './resolvePeers'

export default function (
  depGraph: DependenciesGraph,
  lockfile: Lockfile,
  prefix: string,
  registries: Registries
): {
  newLockfile: Lockfile,
  pendingRequiresBuilds: string[],
} {
  lockfile.packages = lockfile.packages || {}
  const pendingRequiresBuilds = [] as string[]
  for (const depPath of Object.keys(depGraph)) {
    const depNode = depGraph[depPath]
    const [updatedOptionalDeps, updatedDeps] = R.partition(
      (child) => depNode.optionalDependencies.has(depGraph[child.depPath].name),
      Object.keys(depNode.children).map((alias) => ({ alias, depPath: depNode.children[alias] }))
    )
    lockfile.packages[depPath] = toLockfileDependency(pendingRequiresBuilds, depNode.additionalInfo, {
      depGraph,
      depPath,
      prevSnapshot: lockfile.packages[depPath],
      registries,
      registry: dp.getRegistryByPackageName(registries, depNode.name),
      updatedDeps,
      updatedOptionalDeps,
    })
  }
  const warn = (message: string) => logger.warn({ message, prefix })
  return {
    newLockfile: pruneSharedLockfile(lockfile, { warn }),
    pendingRequiresBuilds,
  }
}

function toLockfileDependency (
  pendingRequiresBuilds: string[],
  pkg: {
    deprecated?: string,
    peerDependencies?: Dependencies,
    peerDependenciesMeta?: PeerDependenciesMeta,
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
    registry: string,
    registries: Registries,
    updatedDeps: Array<{alias: string, depPath: string}>,
    updatedOptionalDeps: Array<{alias: string, depPath: string}>,
    depGraph: DependenciesGraph,
    prevSnapshot?: PackageSnapshot,
  }
): PackageSnapshot {
  const depNode = opts.depGraph[opts.depPath]
  const lockfileResolution = toLockfileResolution(
    { name: depNode.name, version: depNode.version },
    opts.depPath,
    depNode.resolution,
    opts.registry
  )
  const newResolvedDeps = updateResolvedDeps(
    opts.prevSnapshot?.dependencies ?? {},
    opts.updatedDeps,
    opts.registries,
    opts.depGraph
  )
  const newResolvedOptionalDeps = updateResolvedDeps(
    opts.prevSnapshot?.optionalDependencies ?? {},
    opts.updatedOptionalDeps,
    opts.registries,
    opts.depGraph
  )
  const result = {
    resolution: lockfileResolution,
  }
  // tslint:disable:no-string-literal
  if (dp.isAbsolute(opts.depPath)) {
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
  if (opts.depPath[0] !== '/' && !depNode.packageId.endsWith(opts.depPath)) {
    result['id'] = depNode.packageId
  }
  if (pkg.peerDependencies) {
    result['peerDependencies'] = pkg.peerDependencies
  }
  if (pkg.peerDependenciesMeta) {
    const normalizedPeerDependenciesMeta = {}
    for (const peer of Object.keys(pkg.peerDependenciesMeta)) {
      if (pkg.peerDependenciesMeta[peer].optional) {
        normalizedPeerDependenciesMeta[peer] = { optional: true }
      }
    }
    if (Object.keys(normalizedPeerDependenciesMeta).length) {
      result['peerDependenciesMeta'] = normalizedPeerDependenciesMeta
    }
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
  if (depNode.hasBin) {
    result['hasBin'] = true
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
  } else if (depNode.requiresBuild !== undefined) {
    if (depNode.requiresBuild) {
      result['requiresBuild'] = true
    }
  } else {
    pendingRequiresBuilds.push(opts.depPath)
  }
  depNode.requiresBuild = result['requiresBuild']
  // tslint:enable:no-string-literal
  return result
}

// previous resolutions should not be removed from lockfile
// as installation might not reanalize the whole dependency graph
// the `depth` property defines how deep should dependencies be checked
function updateResolvedDeps (
  prevResolvedDeps: ResolvedDependencies,
  updatedDeps: Array<{alias: string, depPath: string}>,
  registries: Registries,
  depGraph: DependenciesGraph
) {
  const newResolvedDeps = R.fromPairs<string>(
    updatedDeps
      .map(({ alias, depPath }): R.KeyValuePair<string, string> => {
        const depNode = depGraph[depPath]
        return [
          alias,
          depPathToRef(depNode.depPath, {
            alias,
            realName: depNode.name,
            registries,
            resolution: depNode.resolution,
          }),
        ]
      })
  )
  return R.merge(
    prevResolvedDeps,
    newResolvedDeps
  )
}

function toLockfileResolution (
  pkg: {
    name: string,
    version: string,
  },
  depPath: string,
  resolution: Resolution,
  registry: string
): LockfileResolution {
  // tslint:disable:no-string-literal
  if (dp.isAbsolute(depPath) || resolution.type !== undefined || !resolution['integrity']) {
    return resolution as LockfileResolution
  }
  const base = registry !== resolution['registry'] ? { registry: resolution['registry'] } : {}
  // Sometimes packages are hosted under non-standard tarball URLs.
  // For instance, when they are hosted on npm Enterprise. See https://github.com/pnpm/pnpm/issues/867
  // Or in othere weird cases, like https://github.com/pnpm/pnpm/issues/1072
  const expectedTarball = getNpmTarballUrl(pkg.name, pkg.version, { registry })
  const actualTarball = resolution['tarball'].replace('%2f', '/')
  if (removeProtocol(expectedTarball) !== removeProtocol(actualTarball)) {
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

function removeProtocol (url: string) {
  return url.split('://')[1]
}

function relativeTarball (tarball: string, registry: string) {
  // It is important to save the tarball URL as "relative-path" (without the leading '/').
  // Sometimes registries are located in a subdirectory of a website.
  // For instance, https://mycompany.jfrog.io/mycompany/api/npm/npm-local/
  // So the tarball location should be relative to the directory,
  // it is not an absolute-path reference.
  // So we add @mycompany/mypackage/-/@mycompany/mypackage-2.0.0.tgz
  // not /@mycompany/mypackage/-/@mycompany/mypackage-2.0.0.tgz
  // Related issue: https://github.com/pnpm/pnpm/issues/1827
  if (tarball.substr(0, registry.length) === registry) {
    return tarball.substr(registry.length)
  }
  return tarball
}
