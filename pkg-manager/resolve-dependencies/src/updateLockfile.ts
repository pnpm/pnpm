import { logger } from '@pnpm/logger'
import {
  type LockfileObject,
  type LockfileResolution,
  type PackageSnapshot,
  pruneSharedLockfile,
} from '@pnpm/lockfile.pruner'
import { type Resolution } from '@pnpm/resolver-base'
import { type DepPath, type Registries } from '@pnpm/types'
import * as dp from '@pnpm/dependency-path'
import getNpmTarballUrl from 'get-npm-tarball-url'
import { type KeyValuePair } from 'ramda'
import { partition } from 'ramda'
import { depPathToRef } from './depPathToRef.js'
import { type ResolvedPackage } from './resolveDependencies.js'
import { type DependenciesGraph } from './index.js'

export function updateLockfile (
  { dependenciesGraph, lockfile, prefix, registries, lockfileIncludeTarballUrl }: {
    dependenciesGraph: DependenciesGraph
    lockfile: LockfileObject
    prefix: string
    registries: Registries
    lockfileIncludeTarballUrl?: boolean
  }
): LockfileObject {
  lockfile.packages = lockfile.packages ?? {}
  for (const [depPath, depNode] of Object.entries(dependenciesGraph)) {
    const [updatedOptionalDeps, updatedDeps] = partition(
      (child) => depNode.optionalDependencies.has(child.alias) || depNode.peerDependencies[child.alias]?.optional === true,
      Object.entries<DepPath>(depNode.children).map(([alias, depPath]) => ({ alias, depPath }))
    )
    lockfile.packages[depPath as DepPath] = toLockfileDependency(depNode, {
      depGraph: dependenciesGraph,
      depPath,
      prevSnapshot: lockfile.packages[depPath as DepPath],
      registries,
      registry: dp.getRegistryByPackageName(registries, depNode.name),
      updatedDeps,
      updatedOptionalDeps,
      lockfileIncludeTarballUrl,
    })
  }
  const warn = (message: string) => {
    logger.warn({ message, prefix })
  }
  return pruneSharedLockfile(lockfile, { warn, dependenciesGraph })
}

function toLockfileDependency (
  pkg: ResolvedPackage & { transitivePeerDependencies: Set<string> },
  opts: {
    depPath: string
    registry: string
    registries: Registries
    updatedDeps: Array<{ alias: string, depPath: DepPath }>
    updatedOptionalDeps: Array<{ alias: string, depPath: DepPath }>
    depGraph: DependenciesGraph
    prevSnapshot?: PackageSnapshot
    lockfileIncludeTarballUrl?: boolean
  }
): PackageSnapshot {
  const lockfileResolution = toLockfileResolution(
    { name: pkg.name, version: pkg.version },
    pkg.resolution,
    opts.registry,
    opts.lockfileIncludeTarballUrl
  )

  const newResolvedDeps = updateResolvedDeps(
    opts.updatedDeps,
    opts.depGraph
  )
  const newResolvedOptionalDeps = updateResolvedDeps(
    opts.updatedOptionalDeps,
    opts.depGraph
  )
  const result = {
    resolution: lockfileResolution,
  } as PackageSnapshot
  if (opts.depPath.includes(':')) {
    // There is no guarantee that a non-npmjs.org-hosted package is going to have a version field.
    // Also, for local directory dependencies, the version is not needed.
    if (
      pkg.version &&
      (
        !('type' in lockfileResolution) ||
        lockfileResolution.type !== 'directory'
      )
    ) {
      result['version'] = pkg.version
    }
  }
  if (Object.keys(newResolvedDeps).length > 0) {
    result['dependencies'] = newResolvedDeps
  }
  if (Object.keys(newResolvedOptionalDeps).length > 0) {
    result['optionalDependencies'] = newResolvedOptionalDeps
  }
  if (pkg.optional) {
    result['optional'] = true
  }
  if (pkg.transitivePeerDependencies.size) {
    result['transitivePeerDependencies'] = Array.from(pkg.transitivePeerDependencies).sort()
  }
  if (Object.keys(pkg.peerDependencies ?? {}).length > 0) {
    const peerPkgs: Record<string, string> = {}
    const normalizedPeerDependenciesMeta: Record<string, { optional: true }> = {}
    for (const [peer, { version, optional }] of Object.entries(pkg.peerDependencies)) {
      peerPkgs[peer] = version
      if (optional) {
        normalizedPeerDependenciesMeta[peer] = { optional: true }
      }
    }
    result['peerDependencies'] = peerPkgs
    if (Object.keys(normalizedPeerDependenciesMeta).length > 0) {
      result['peerDependenciesMeta'] = normalizedPeerDependenciesMeta
    }
  }
  if (pkg.additionalInfo.engines != null) {
    for (const [engine, version] of Object.entries(pkg.additionalInfo.engines)) {
      if (version === '*') continue
      result.engines = result.engines ?? {} as any // eslint-disable-line @typescript-eslint/no-explicit-any
      result.engines![engine] = version
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
  if (
    Array.isArray(pkg.additionalInfo.bundledDependencies) ||
    pkg.additionalInfo.bundledDependencies === true
  ) {
    result['bundledDependencies'] = pkg.additionalInfo.bundledDependencies
  } else if (
    Array.isArray(pkg.additionalInfo.bundleDependencies) ||
    pkg.additionalInfo.bundleDependencies === true
  ) {
    result['bundledDependencies'] = pkg.additionalInfo.bundleDependencies
  }
  if (pkg.additionalInfo.deprecated) {
    result['deprecated'] = pkg.additionalInfo.deprecated
  }
  if (pkg.hasBin) {
    result['hasBin'] = true
  }
  if (pkg.patch) {
    result['patched'] = true
  }
  return result
}

function updateResolvedDeps (
  updatedDeps: Array<{ alias: string, depPath: DepPath }>,
  depGraph: DependenciesGraph
): Record<string, string> {
  return Object.fromEntries(
    updatedDeps
      .map(({ alias, depPath }): KeyValuePair<string, string> => {
        if (depPath.startsWith('link:')) {
          return [alias, depPath]
        }
        const depNode = depGraph[depPath]
        return [
          alias,
          depPathToRef(depPath, {
            alias,
            realName: depNode.name,
          }),
        ]
      })
  )
}

function toLockfileResolution (
  pkg: {
    name: string
    version: string
  },
  resolution: Resolution,
  registry: string,
  lockfileIncludeTarballUrl?: boolean
): LockfileResolution {
  if (resolution.type !== undefined || !resolution['integrity']) {
    return resolution as LockfileResolution
  }
  if (lockfileIncludeTarballUrl) {
    return {
      integrity: resolution['integrity'],
      tarball: resolution['tarball'],
    }
  }
  if (lockfileIncludeTarballUrl === false) {
    return {
      integrity: resolution['integrity'],
    }
  }
  // Sometimes packages are hosted under non-standard tarball URLs.
  // For instance, when they are hosted on npm Enterprise. See https://github.com/pnpm/pnpm/issues/867
  // Or in other weird cases, like https://github.com/pnpm/pnpm/issues/1072
  const expectedTarball = getNpmTarballUrl(pkg.name, pkg.version, { registry })
  const actualTarball = resolution['tarball'].replace('%2f', '/')
  if (removeProtocol(expectedTarball) !== removeProtocol(actualTarball)) {
    return {
      integrity: resolution['integrity'],
      tarball: resolution['tarball'],
    }
  }
  return {
    integrity: resolution['integrity'],
  }
}

function removeProtocol (url: string): string {
  return url.split('://')[1]
}
