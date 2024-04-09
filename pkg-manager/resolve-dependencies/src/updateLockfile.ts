import { logger } from '@pnpm/logger'
import {
  type Lockfile,
  type LockfileResolution,
  type PackageSnapshot,
  pruneSharedLockfile,
} from '@pnpm/prune-lockfile'
import { type DirectoryResolution, type Resolution } from '@pnpm/resolver-base'
import { type Registries } from '@pnpm/types'
import * as dp from '@pnpm/dependency-path'
import getNpmTarballUrl from 'get-npm-tarball-url'
import { type KeyValuePair } from 'ramda'
import partition from 'ramda/src/partition'
import { depPathToRef } from './depPathToRef'
import { type ResolvedPackage } from './resolveDependencies'
import { type DependenciesGraph } from '.'

export function updateLockfile (
  { dependenciesGraph, lockfile, prefix, registries, lockfileIncludeTarballUrl }: {
    dependenciesGraph: DependenciesGraph
    lockfile: Lockfile
    prefix: string
    registries: Registries
    lockfileIncludeTarballUrl?: boolean
  }
): Lockfile {
  lockfile.packages = lockfile.packages ?? {}
  for (const [depPath, depNode] of Object.entries(dependenciesGraph)) {
    const [updatedOptionalDeps, updatedDeps] = partition(
      (child) => depNode.optionalDependencies.has(child.alias),
      Object.entries(depNode.children).map(([alias, depPath]) => ({ alias, depPath }))
    )
    lockfile.packages[depPath] = toLockfileDependency(depNode, {
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
  const warn = (message: string) => {
    logger.warn({ message, prefix })
  }
  return pruneSharedLockfile(lockfile, { warn })
}

function toLockfileDependency (
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
    if (pkg.version && (lockfileResolution as DirectoryResolution).type !== 'directory') {
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
  if (pkg.patchFile) {
    result['patched'] = true
  }
  return result
}

function updateResolvedDeps (
  updatedDeps: Array<{ alias: string, depPath: string }>,
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
          depPathToRef(depNode.depPath, {
            alias,
            realName: depNode.name,
            resolution: depNode.resolution,
          }),
        ]
      })
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
  if (resolution.type !== undefined || !resolution['integrity']) {
    return resolution as LockfileResolution
  }
  if (lockfileIncludeTarballUrl) {
    return {
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
