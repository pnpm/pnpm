import { logger } from '@pnpm/logger'
import {
  Lockfile,
  LockfileResolution,
  PackageSnapshot,
  pruneSharedLockfile,
  ResolvedDependencies,
} from '@pnpm/prune-lockfile'
import { Resolution } from '@pnpm/resolver-base'
import { Registries } from '@pnpm/types'
import * as dp from 'dependency-path'
import getNpmTarballUrl from 'get-npm-tarball-url'
import { KeyValuePair } from 'ramda'
import isEmpty from 'ramda/src/isEmpty'
import fromPairs from 'ramda/src/fromPairs'
import mergeRight from 'ramda/src/mergeRight'
import partition from 'ramda/src/partition'
import { depPathToRef } from './depPathToRef'
import { ResolvedPackage } from './resolveDependencies'
import { DependenciesGraph } from '.'

export function updateLockfile (
  { dependenciesGraph, lockfile, prefix, registries, lockfileIncludeTarballUrl }: {
    dependenciesGraph: DependenciesGraph
    lockfile: Lockfile
    prefix: string
    registries: Registries
    lockfileIncludeTarballUrl?: boolean
  }
): {
    newLockfile: Lockfile
    pendingRequiresBuilds: string[]
  } {
  lockfile.packages = lockfile.packages ?? {}
  const pendingRequiresBuilds = [] as string[]
  for (const [depPath, depNode] of Object.entries(dependenciesGraph)) {
    const [updatedOptionalDeps, updatedDeps] = partition(
      (child) => depNode.optionalDependencies.has(child.alias),
      Object.keys(depNode.children).map((alias) => ({ alias, depPath: depNode.children[alias] }))
    )
    lockfile.packages[depPath] = toLockfileDependency(pendingRequiresBuilds, depNode, {
      depGraph: dependenciesGraph,
      depPath,
      prevSnapshot: lockfile.packages[depPath],
      registries,
      registry: dp.getRegistryByPackageName(registries, depNode.name),
      updatedDeps,
      updatedOptionalDeps,
      lockfileIncludeTarballUrl,
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
  pkg: ResolvedPackage & { transitivePeerDependencies: Set<string> },
  opts: {
    depPath: string
    registry: string
    registries: Registries
    updatedDeps: Array<{ alias: string, depPath: string }>
    updatedOptionalDeps: Array<{ alias: string, depPath: string }>
    depGraph: DependenciesGraph
    prevSnapshot?: PackageSnapshot
    lockfileIncludeTarballUrl?: boolean
  }
): PackageSnapshot {
  const lockfileResolution = toLockfileResolution(
    { id: pkg.id, name: pkg.name, version: pkg.version },
    opts.depPath,
    pkg.resolution,
    opts.registry,
    opts.lockfileIncludeTarballUrl
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
  if (dp.isAbsolute(opts.depPath)) {
    result['name'] = pkg.name

    // There is no guarantee that a non-npmjs.org-hosted package
    // is going to have a version field
    if (pkg.version) {
      result['version'] = pkg.version
    }
  }
  if (!isEmpty(newResolvedDeps)) {
    result['dependencies'] = newResolvedDeps
  }
  if (!isEmpty(newResolvedOptionalDeps)) {
    result['optionalDependencies'] = newResolvedOptionalDeps
  }
  if (pkg.dev && !pkg.prod) {
    result['dev'] = true
  } else if (pkg.prod && !pkg.dev) {
    result['dev'] = false
  }
  if (pkg.optional) {
    result['optional'] = true
  }
  if (opts.depPath[0] !== '/' && !pkg.id.endsWith(opts.depPath)) {
    result['id'] = pkg.id
  }
  if (!isEmpty(pkg.peerDependencies ?? {})) {
    result['peerDependencies'] = pkg.peerDependencies
  }
  if (pkg.transitivePeerDependencies.size) {
    result['transitivePeerDependencies'] = Array.from(pkg.transitivePeerDependencies).sort()
  }
  if (pkg.peerDependenciesMeta != null) {
    const normalizedPeerDependenciesMeta = {}
    for (const peer of Object.keys(pkg.peerDependenciesMeta)) {
      if (pkg.peerDependenciesMeta[peer].optional) {
        normalizedPeerDependenciesMeta[peer] = { optional: true }
      }
    }
    if (Object.keys(normalizedPeerDependenciesMeta).length > 0) {
      result['peerDependenciesMeta'] = normalizedPeerDependenciesMeta
    }
  }
  if (pkg.additionalInfo.engines != null) {
    for (const engine of Object.keys(pkg.additionalInfo.engines)) {
      if (pkg.additionalInfo.engines[engine] === '*') continue
      result['engines'] = result['engines'] || {}
      result['engines'][engine] = pkg.additionalInfo.engines[engine]
    }
  }
  if (pkg.additionalInfo.cpu != null) {
    result['cpu'] = pkg.additionalInfo.cpu
  }
  if (pkg.additionalInfo.os != null) {
    result['os'] = pkg.additionalInfo.os
  }
  if (pkg.additionalInfo.libc != null) {
    result['libc'] = pkg.additionalInfo.libc
  }
  if (Array.isArray(pkg.additionalInfo.bundledDependencies) || Array.isArray(pkg.additionalInfo.bundleDependencies)) {
    result['bundledDependencies'] = pkg.additionalInfo.bundledDependencies ?? pkg.additionalInfo.bundleDependencies
  }
  if (pkg.additionalInfo.deprecated) {
    result['deprecated'] = pkg.additionalInfo.deprecated
  }
  if (pkg.hasBin) {
    result['hasBin'] = true
  }
  if (pkg.patchFile) {
    result['patched'] = true
  }
  const requiresBuildIsKnown = typeof pkg.requiresBuild === 'boolean'
  let pending = false
  if (requiresBuildIsKnown) {
    if (pkg.requiresBuild) {
      result['requiresBuild'] = true
    }
  } else if (opts.prevSnapshot != null) {
    if (opts.prevSnapshot.requiresBuild) {
      result['requiresBuild'] = opts.prevSnapshot.requiresBuild
    }
    if (opts.prevSnapshot.prepare) {
      result['prepare'] = opts.prevSnapshot.prepare
    }
  } else if (pkg.prepare) {
    result['prepare'] = true
    result['requiresBuild'] = true
  } else {
    pendingRequiresBuilds.push(opts.depPath)
    pending = true
  }
  if (!requiresBuildIsKnown && !pending) {
    pkg.requiresBuild['resolve'](result['requiresBuild'] ?? false)
  }
  /* eslint-enable @typescript-eslint/dot-notation */
  return result
}

// previous resolutions should not be removed from lockfile
// as installation might not reanalyze the whole dependency graph
// the `depth` property defines how deep should dependencies be checked
function updateResolvedDeps (
  prevResolvedDeps: ResolvedDependencies,
  updatedDeps: Array<{ alias: string, depPath: string }>,
  registries: Registries,
  depGraph: DependenciesGraph
) {
  const newResolvedDeps = fromPairs<string>(
    updatedDeps
      .map(({ alias, depPath }): KeyValuePair<string, string> => {
        if (depPath.startsWith('link:')) {
          return [alias, depPath]
        }
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
  return mergeRight(
    prevResolvedDeps,
    newResolvedDeps
  )
}

function toLockfileResolution (
  pkg: {
    id: string
    name: string
    version: string
  },
  depPath: string,
  resolution: Resolution,
  registry: string,
  lockfileIncludeTarballUrl?: boolean
): LockfileResolution {
  if (dp.isAbsolute(depPath) || resolution.type !== undefined || !resolution['integrity']) {
    return resolution as LockfileResolution
  }
  const base = registry !== resolution['registry'] ? { registry: resolution['registry'] } : {}
  if (lockfileIncludeTarballUrl) {
    return {
      ...base,
      integrity: resolution['integrity'],
      tarball: resolution['tarball'],
    }
  }
  // Sometimes packages are hosted under non-standard tarball URLs.
  // For instance, when they are hosted on npm Enterprise. See https://github.com/pnpm/pnpm/issues/867
  // Or in other weird cases, like https://github.com/pnpm/pnpm/issues/1072
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
  /* eslint-enable @typescript-eslint/dot-notation */
}

function removeProtocol (url: string) {
  return url.split('://')[1]
}

export function relativeTarball (tarball: string, registry: string) {
  // It is important to save the tarball URL as "relative-path" (without the leading '/').
  // Sometimes registries are located in a subdirectory of a website.
  // For instance, https://mycompany.jfrog.io/mycompany/api/npm/npm-local/
  // So the tarball location should be relative to the directory,
  // it is not an absolute-path reference.
  // So we add @mycompany/mypackage/-/@mycompany/mypackage-2.0.0.tgz
  // not /@mycompany/mypackage/-/@mycompany/mypackage-2.0.0.tgz
  // Related issue: https://github.com/pnpm/pnpm/issues/1827
  if (tarball.slice(0, registry.length) !== registry) {
    return tarball
  }
  const relative = tarball.slice(registry.length)
  if (relative[0] === '/') return relative.substring(1)
  return relative
}
